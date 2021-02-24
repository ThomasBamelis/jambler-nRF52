mod state;
mod util;
mod hardware_traits;

// Re-export hardware implementations for user
use heapless::Vec;
pub use hardware_traits::nrf52840;

use heapless::{consts::*, String};
use state::IntervalTimerRequirements;
use state::StateConfig;
use state::StateStore;
use state::{StateParameters, StateReturn};
use hardware_traits::*;

use rtt_target::rprintln;

/// The generic implementation of the vulnerability.
/// This is supposed to hold the BLE vulnerability code, not chip specific code.
/// It will hold a field for every possible state, as you cannot abstract it to just the trait because this means this field could change size (the state struct size) and I have no heap. This is the simplest solution.
/// 
/// The JamBLEr controller is responsible for receiving tasks and following the correct state transitions for that task.
/// Whether the state itself indicates it wants to transition or because required background work is done.
/// The controller is responsible for proper task execution in the same way that the state store is responsible for proper state execution.
pub struct JamBLEr<H: JamBLErHal, T: JamBLErTimer, I: JamBLErIntervalTimer> {
    jammer_hal: H,
    jammer_timer: T,
    jammer_interval_timer: I,
    state_store: StateStore,
    current_task: JamBLErTask,
}

/// TODO move to state.rs
#[derive(Clone, Debug)]
pub enum JamBLErState {
    Idle,
    DiscoveringAAs,
    HarvestingPackets,
}

/// Use this to pass parameters, which you can use in the state conf.
/// For example SniffAA(access address)
/// While JamblerState might have 5 states for recovering a connection given an access address
/// this will only contain a recover connection(aa) enum
///
/// One task is basically subdevided into multiple jammer states
/// 
/// See the diagram about task state diagrams to better understand this.
#[derive(Clone, Debug)]
pub enum JamBLErTask {
    UserInterrupt,
    Idle,
    DiscoverAas,
    Jam,
}


impl<H: JamBLErHal, T: JamBLErTimer, I: JamBLErIntervalTimer> JamBLEr<H, T, I> {
    pub fn new(
        mut jammer_hal: H,
        mut jammer_timer: T,
        jammer_interval_timer: I,
    ) -> JamBLEr<H, T, I> {
        // Start the timer
        jammer_timer.start();
        JamBLEr {
            jammer_hal,
            jammer_timer,
            jammer_interval_timer,
            state_store: StateStore::new(),
            current_task: JamBLErTask::Idle,
        }
    }

    /// Should be called from main or whatever to make JamBLEr do what user wants.
    pub fn execute_task(&mut self, task: JamBLErTask) {
        rprintln!("Received task {:?}", task);
        let prev_task = self.current_task.clone();
        self.current_task = task;
        // always start to idle first, because any state goes can transition to idle
        //self.state_transition(JamBLErState::Idle);
        // Not necessary because for a user to stop the current task,
        // they  have to interrupt, which will put the jambler in idle

        // These transition the jambler into the start state of the given task.
        // The current state should always be idle, except for a user interrupt.
        match self.current_task {
            JamBLErTask::UserInterrupt => {
                self.user_interrupt();
            }
            JamBLErTask::Idle => {
                self.state_transition(JamBLErState::Idle, StateConfig::new());
            }
            JamBLErTask::DiscoverAas => {
                // TODO specify all in command or I2C communication
                let mut config = StateConfig::new();
                // for now, listen on legacy phy, all data channels and switch every 3 seconds
                config.phy = Some(BlePHY::Uncoded1M);
                config.interval = Some(3 * 1_000_000);
                let mut cc : Vec<u8, U64> = Vec::new();
                for i in 0..=36 {
                    cc.push(i).unwrap();
                }
                config.channel_chain = Some(cc);
                
                self.state_transition(JamBLErState::DiscoveringAAs, config);
            }
            JamBLErTask::Jam => {
                // TODO specify all in command or I2C communication
                let mut config = StateConfig::new();

                // for now, listen on legacy phy, all data channels 
                // max interval for 100, advertising AA
                config.access_address = Some(0xAF9ABB1B);
                config.phy = Some(BlePHY::CodedS8);
                config.slave_phy = Some(BlePHY::CodedS8);
                config.interval = Some(4_000_000);
                let mut cc : Vec<u8, U64> = Vec::new();
                for i in 24..=24 {
                    cc.push(i).unwrap();
                }
                config.channel_chain = Some(cc);

                config.number_of_intervals = Some(1);

                // no crc init
                config.crc_init = Some(0x555555);

                // interval timer 500 ppm so to speak
                config.interval_timer_ppm = Some(500);

                // interval timer 500 ppm so to speak
                config.long_term_timer_ppm = Some(500);

                self.state_transition(JamBLErState::HarvestingPackets, config);
            }
        };
    }

    /// What happens on a user interrupt.
    /// For now, just idle.
    fn user_interrupt(&mut self) {
        self.state_transition(JamBLErState::Idle, StateConfig::new());
    }

    /// Helper function for setting the interval timer.
    #[inline]
    fn set_interval_timer(&mut self, req: &IntervalTimerRequirements) {
        rprintln!("Setting interval timer: {:?}", &req);
        match req {
            IntervalTimerRequirements::NoChanges => {}
            IntervalTimerRequirements::NoIntervalTimer => {
                self.jammer_interval_timer.reset();
            }
            IntervalTimerRequirements::Countdown(interval) => {
                self.jammer_interval_timer.config(*interval, false);
                self.jammer_interval_timer.start();
            }
            IntervalTimerRequirements::Periodic(interval) => {
                self.jammer_interval_timer.config(*interval, true);
                self.jammer_interval_timer.start();
            }
        }
    }

    /// A state transition can reset the timer twice.
    /// It is first reset to prevent any new timer interrupts.
    /// The set_interval_timer will also reset the timer if it starts or reset the timer.
    /// Better safe than sorry for now.
    pub fn state_transition(&mut self, new_state: JamBLErState, config: StateConfig) {
        // Disable interval timer to prevent it preempting this in the middle.
        self.jammer_interval_timer.reset();

        rprintln!(
            "Transitioning state: {:?} -> {:?}",
            self.state_store.get_current_state(),
            &new_state
        );

        // Get current time
        let current_time = self.jammer_timer.get_time_micro_seconds();

        // construct the parameters
        let parameters = &mut StateParameters{
            radio: &mut self.jammer_hal, 
            current_time, 
            config: Some(config)
        };

        // Dispatch transition to the state store
        let transition_result = self.state_store.state_transition(
            new_state,
            parameters
        );

        // Check for errors and any timing requirements
        match transition_result {
            Ok(state_return) => {
                if let Some(ret) = &state_return {
                    if let Some(timing_requirements) = &ret.timing_requirements {
                        self.set_interval_timer(timing_requirements);
                    }
                    // TODO any output or something
                }
            }
            Err(state_error) => {
                rprintln!("ERROR: invalid state transition\n{:?}", state_error);
                panic!()
            }
        }
    }

    /// Radio interrupt received, dispatch it to the state
    #[inline]
    pub fn handle_radio_interrupt(&mut self) -> Option<JamBLErReturn> {
        // Get current time
        let current_time = self.jammer_timer.get_time_micro_seconds();
        // construct the parameters
        let parameters = &mut StateParameters{
            radio: &mut self.jammer_hal, 
            current_time, 
            config: None
        };
        // Dispatch to state
        let state_return = self
            .state_store
            .handle_radio_interrupt(parameters);


        // process return
        match state_return {
            Ok(ok_state_return) => {
                match ok_state_return {
                    Some(ret) => {
                        // Obey timing requirements ASAP
                        if let Some(timing_requirements) = &ret.timing_requirements {
                            self.set_interval_timer(timing_requirements);
                        }
    
                        // Process the return value.
                        self.process_state_return_value(ret)
                    },
                    None => {
                        None
                    }
                }
            },
            Err(state_error) => {
                rprintln!("ERROR: by state in radio interrupt\n{:?}", state_error);
                panic!()
            }
        }
    }

    /// Received interval timer interrupt, dispatch it to the state.
    #[inline]
    pub fn handle_interval_timer_interrupt(&mut self) -> Option<JamBLErReturn> {
        // Necessary. At least for nrf because event needs to be reset.
        self.jammer_interval_timer.interrupt_handler();
        // Get current time;
        let current_time = self.jammer_timer.get_time_micro_seconds();


        // construct the parameters
        let parameters = &mut StateParameters{
            radio: &mut self.jammer_hal, 
            current_time, 
            config: None
        };

        // Dispatch it to the state
        let state_return = self
            .state_store
            .handle_interval_timer_interrupt(parameters);

        // process return
        match state_return {
            Ok(ok_state_return) => {
                match ok_state_return {
                    Some(ret) => {
                        // Obey timing requirements ASAP
                        if let Some(timing_requirements) = &ret.timing_requirements {
                            self.set_interval_timer(timing_requirements);
                        }
    
                        // Process the return value.
                        self.process_state_return_value(ret)
                    },
                    None => {
                        None
                    }
                }
            },
            Err(state_error) => {
                rprintln!("ERROR: by state in interval timer interrupt\n{:?}", state_error);
                panic!()
            }
        }
    }

    /// Handler for the long term interrupt timer for when it wraps
    #[inline]
    pub fn handle_timer_interrupt(&mut self) {
        self.jammer_timer.interrupt_handler();
    }

    /// Processes the return from a state, regardless from which interrupt.
    /// The state is telling the controller something here and should act accordingly.
    #[inline]
    fn process_state_return_value(&mut self, return_type : StateReturn) -> Option<JamBLErReturn> {
        //TODO
        // TODO will most likely need a return t
        if let Some(m) = return_type.state_message {
            rprintln!("State message: {:?}",m);
        }
        Some(JamBLErReturn::NoReturn)
    }
}


#[derive(Debug, Clone)]
pub enum JamBLErReturn {
    OutputString(String<U256>),
    NoReturn,
}


#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BlePHY {
    Uncoded1M,
    Uncoded2M,
    CodedS2,
    CodedS8,
}
