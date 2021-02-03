/// Jammer states trait
use heapless::{
    consts::*,
    String
};


use super::{JamBLErHal, JamBLErState, BlePHY};


pub struct StateConfig {
    pub phy : Option<BlePHY>,
    pub access_address : Option<u32>,
    pub channel_map : Option<[bool; 37]>,
    pub crc_init : Option<u32>,
    pub csa_version : Option<u8>,
    pub channel : Option<u8>,
    pub hop_interval : Option<u32>,
    pub hop_increment : Option<u32>,
    pub initial_counter_value : Option<u32>,
    pub counter : Option<u32>,
    pub previous_state : Option<JamBLErState>
}

impl StateConfig {
    pub fn new() -> StateConfig {
        StateConfig {
             phy : None,
             access_address : None,
             channel_map : None,
             crc_init : None,
             csa_version : None,
             channel : None,
             hop_interval : None,
             hop_increment : None,
             initial_counter_value : None,
             counter : None,
             previous_state : None
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
    fn config(&mut self, parameters : StateConfig) -> bool;

    /// Functions as a reset + start!
    /// Every state should have a config method which you should call before this one.
    fn initialise(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64) -> IntervalTimerRequirements;

    fn launch(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64);

    fn stop(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64);

    /// Handle an interrupt
    fn handle_interrupt(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64) -> (HandlerReturn, IntervalTimerRequirements) ;

    
    fn handle_interval_timer_interrupt(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64)  -> (HandlerReturn, IntervalTimerRequirements) ;
    
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
pub enum IntervalTimerRequirements {
    NoIntervalTimer,
    NoChanges,
    Periodic(u32),
    Countdown(u32),
}

pub mod idle;
pub mod discover_aas;

/// Will hold a struct of every possible state.
/// Necessary to avoid dynamic allocation but leverage polymorphism
/// and use the state GOF pattern.
/// It will have a function that will return a reference to the right jammerstate implementation given the corresponding JamBLErState enum.
pub struct StateStore {
    pub idle : idle::Idle,
    pub discover_aas : discover_aas::DiscoverAas,
}

impl StateStore {
    pub fn new() -> StateStore {
        StateStore {
            idle : idle::Idle::new(),
            discover_aas : discover_aas::DiscoverAas::new(),
        }
    }
}

