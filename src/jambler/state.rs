/// Jammer states trait
/// This will handle the ugly truth of avoiding dynamic dispatch.
use heapless::{consts::*, String};

use super::{BlePHY, JamBLErHal, JamBLErState};

/// Possible parameters a state might need to configure itself.
pub struct StateConfig {
    pub phy: Option<BlePHY>,
    pub access_address: Option<u32>,
    pub channel_map: Option<[bool; 37]>,
    pub crc_init: Option<u32>,
    pub csa_version: Option<u8>,
    pub channel: Option<u8>,
    pub hop_interval: Option<u32>,
    pub hop_increment: Option<u32>,
    pub initial_counter_value: Option<u32>,
    pub counter: Option<u32>,
    pub previous_state: Option<JamBLErState>,
}

impl StateConfig {
    /// Default constructor for quickly initialising a config
    /// without any parameters.
    pub fn new() -> StateConfig {
        StateConfig {
            phy: None,
            access_address: None,
            channel_map: None,
            crc_init: None,
            csa_version: None,
            channel: None,
            hop_interval: None,
            hop_increment: None,
            initial_counter_value: None,
            counter: None,
            previous_state: None,
        }
    }
}

/// A JammerState has to be able to be started and stopped.
/// Inbetween that, it will handle all the radio interrupts.
/// It can also ask for a periodic timer interrupt or an interrupt after a certain interval.
/// This is necessary for for example changing interval after a certain period or for deciding you lost the connection.
///
/// Telling the controller what you want or what you know can only happen at interaction points.
/// These are after initialise, handle_interrupt and handle_interval_interrupt
///
/// So how it works:
/// 1) call config on the state, then initialise which returns timing requirements
/// 2) The controller should start those timing requirements and then launch.
/// 3) The state will return a HandlerReturn return Value and timing requirements on every handle_interrupt (radio  interrupt) or handle_interval_timer_interrrupt
/// 4) Before this state is left, the stop function will be called, giving the state the opportunity to do cleanup.
pub trait JammerState {
    fn new() -> Self;

    /// Returns false if a parameter was missing, will do nothing in that case.
    fn config(&mut self, parameters: StateConfig) -> Result<(), StateError>;

    /// Functions as a reset + start!
    /// Every state should have a config method which you should call before this one.
    fn initialise(
        &mut self,
        radio: &mut impl JamBLErHal,
        instant_in_microseconds: u64,
    ) -> IntervalTimerRequirements;

    fn launch(&mut self, radio: &mut impl JamBLErHal, instant_in_microseconds: u64);

    fn stop(&mut self, radio: &mut impl JamBLErHal, instant_in_microseconds: u64);

    /// Handle a radio interrupt.
    /// ALWAYS INLINE IN IMPLEMENTATION!
    fn handle_radio_interrupt(
        &mut self,
        radio: &mut impl JamBLErHal,
        instant_in_microseconds: u64,
    ) -> (HandlerReturn, IntervalTimerRequirements);

    /// Handle an interval timer interrupt.
    /// ALWAYS INLINE IN IMPLEMENTATION!
    fn handle_interval_timer_interrupt(
        &mut self,
        radio: &mut impl JamBLErHal,
        instant_in_microseconds: u64,
    ) -> (HandlerReturn, IntervalTimerRequirements);

    /// Is it valid to go from the self state to the new state.
    /// self -> new_state valid?
    fn is_valid_transition_to(&mut self, new_state: &JamBLErState) -> Result<(), StateError>;

    /// Is it valid to go to the self state from the old_state
    /// new_state -> self valid?
    fn is_valid_transition_from(&mut self, old_state: &JamBLErState) -> Result<(), StateError>;
}

/// Enum for returning feedback or a task from the state functions.
/// An enum in memory is always the size of its biggest variant,
/// that is why we can return it. Returning an impl which some structs implement
/// would not work because they can have different sizes at runtime which is not allowed.
/// Remember, no dynamic allocation.
pub enum HandlerReturn {
    OutputString(String<U256>),
    NoReturn,
}

/// Indicates to the controller which timing requirements you want after an interaction.
#[derive(Clone,Debug)]
pub enum IntervalTimerRequirements {
    NoIntervalTimer,
    NoChanges,
    Periodic(u32),
    Countdown(u32),
}

pub mod discover_aas;
pub mod idle;

/// Will hold a struct of every possible state.
/// Necessary to avoid dynamic allocation but leverage polymorphism
/// and use the state GOF pattern.
/// It will have a function that will return a reference to the right jammerstate implementation given the corresponding JamBLErState enum.
pub struct StateStore {
    current_state: JamBLErState,
    pub idle: idle::Idle,
    pub discover_aas: discover_aas::DiscoverAas,
}

/*
macro_rules! transistion_to {
    ($s:expr, $conf:expr, $hal:expr, $time:expr, $it:expr, $p:expr, $i:expr) => {
        $s.config($conf); //  if false rprintln
        $s.initialise(&mut $hal, $time);
        $it = $s.needs_interval_timer();
        $p = $s.needs_periodic_interrupt();
        $i = $s.timer_interval();

        $s.launch(&mut $hal, $time);
    };
}
*/
impl StateStore {
    pub fn new() -> StateStore {
        StateStore {
            current_state: JamBLErState::Idle,
            idle: idle::Idle::new(),
            discover_aas: discover_aas::DiscoverAas::new(),
        }
    }

    pub fn get_current_state(&self) -> JamBLErState {
        self.current_state.clone()
    }

    /// Transitions state in the proper way, only for valid state transitions.
    /// This also serves as a way for me to protect me from myself and easily catch things I did not intend to happen.
    pub fn state_transition(
        &mut self,
        new_state: JamBLErState,
        config: StateConfig,
        radio: &mut impl JamBLErHal,
        instant_in_microseconds: u64,
    ) -> Result<IntervalTimerRequirements, StateError> {
        // We will stop the previous state even though it can crash later in new state
        // However, leaving the system in an invalid state is not bad because it is a crash either way, an invalid transition

        // Check if old -> new is valid for old and stop if if ok
        // The ? will make the function return early.
        match self.current_state {
            JamBLErState::Idle => {
                let state = &mut self.idle;

                // This is identical for every case
                state.is_valid_transition_to(&new_state)?;
                state.stop(radio, instant_in_microseconds);
            }
            JamBLErState::DiscoveringAAs => {
                let state = &mut self.discover_aas;

                state.is_valid_transition_to(&new_state)?;
                state.stop(radio, instant_in_microseconds);
            }
        };

        let timing_requirements;

        // configure the state, initialise it, get its timing requirements
        // and launch it.
        match &new_state {
            JamBLErState::Idle => {
                let state = &mut self.idle;

                // This is identical for every case
                state.is_valid_transition_from(&self.current_state)?;
                state.config(config)?;
                timing_requirements = state.initialise(radio, instant_in_microseconds);
                state.launch(radio, instant_in_microseconds);
            }
            JamBLErState::DiscoveringAAs => {
                let state = &mut self.discover_aas;

                state.is_valid_transition_from(&self.current_state)?;
                state.config(config)?;
                timing_requirements = state.initialise(radio, instant_in_microseconds);
                state.launch(radio, instant_in_microseconds);
            }
        };

        self.current_state = new_state;

        Ok(timing_requirements)
    }

    /// Will dispatch the radio interrupt to the right jammerstate for the current jamblerstate.
    #[inline]
    pub fn handle_radio_interrupt(
        &mut self,
        radio: &mut impl JamBLErHal,
        instant_in_microseconds: u64,
    ) -> (HandlerReturn, IntervalTimerRequirements) {
        match self.current_state {
            JamBLErState::Idle => {
                let state = &mut self.idle;

                // Following is same for every case
                state.handle_radio_interrupt(radio, instant_in_microseconds)
            }
            JamBLErState::DiscoveringAAs => {
                let state = &mut self.discover_aas;

                state.handle_radio_interrupt(radio, instant_in_microseconds)
            }
        }
    }

    /// Will dispatch the interval timer interrupt to the right jammerstate for the current jamblerstate.
    #[inline]
    pub fn handle_interval_timer_interrupt(
        &mut self,
        radio: &mut impl JamBLErHal,
        instant_in_microseconds: u64,
    ) -> (HandlerReturn, IntervalTimerRequirements) {
        match self.current_state {
            JamBLErState::Idle => {
                let state = &mut self.idle;

                // Following is same for every case
                state.handle_interval_timer_interrupt(radio, instant_in_microseconds)
            }
            JamBLErState::DiscoveringAAs => {
                let state = &mut self.discover_aas;

                state.handle_interval_timer_interrupt(radio, instant_in_microseconds)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum StateError {
    InvalidStateTransition(&'static str),
    InvalidConfig(&'static str),
}
