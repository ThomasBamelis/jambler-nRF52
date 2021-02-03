pub mod nrf52840;
pub mod state;

use state::{JammerState, HandlerReturn};
use state::StateStore;
use state::StateConfig;
use state::IntervalTimerRequirements;

use rtt_target::rprintln;






/// The generic implementation of the vulnerability.
/// This is supposed to hold the BLE vulnerability code, not chip specific code.
/// It will hold a field for every possible state, as you cannot abstract it to just the trait because this means this field could change size (the state struct size) and I have no heap. This is the simplest solution.
pub struct JamBLEr<H: JamBLErHal, T: JamBLErTimer, I: JamBLErIntervalTimer> {
    jammer_hal : H,
    state : JamBLErState,
    jammer_timer : T,
    jammer_interval_timer: I,
    state_store : StateStore,
    current_task : JamBLErTask,
    // TODO: lambda as output sink, for this and hal, accepts buffer and outputs it to whatever, returns () (jammer never bothered with failure)
}

#[derive(Clone, Debug)]
pub enum JamBLErState {
    Idle,
    DiscoveringAAs,
}

/// Use this to pass parameters, which you can use in the state conf.
/// For example SniffAA(access address)
/// While JamblerState might have 5 states for recovering a connection given an access address
/// this will only contain a recover connection(aa) enum
/// 
/// One task is basically subdevided into multiple jammer states
#[derive(Clone, Debug)]
pub enum JamBLErTask {
    UserInterrupt,
    Idle,
    DiscoverAas,
}


/*
macro_rules! transistion_to {
    ($s:expr, $conf:expr, $hal:expr, $time:expr, $it:expr, $p:expr, $i:expr) => {
        $s.config($conf); // TODO if false rprintln
        $s.initialise(&mut $hal, $time);
        $it = $s.needs_interval_timer();
        $p = $s.needs_periodic_interrupt();
        $i = $s.timer_interval();

        $s.launch(&mut $hal, $time);
    };
}
*/
impl<H: JamBLErHal, T:JamBLErTimer, I:JamBLErIntervalTimer> JamBLEr<H, T, I> {
    pub fn new(mut jammer_hal : H,mut jammer_timer : T, jammer_interval_timer : I) -> JamBLEr<H, T, I> {
        // Start the timer
        jammer_timer.start();
        // call idle start
        let mut state_store = StateStore::new();
        let config = StateConfig::new();
        state_store.idle.config(config);
        state_store.idle.initialise(&mut jammer_hal, 0);
        state_store.idle.launch(&mut jammer_hal, 0);
        JamBLEr {
            jammer_hal,
            state: JamBLErState::Idle,
            jammer_timer,
            jammer_interval_timer,
            state_store : StateStore::new(),
            current_task : JamBLErTask::Idle,
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
            },
            JamBLErTask::Idle => {
                self.state_transition(JamBLErState::Idle);
            },
            JamBLErTask::DiscoverAas => {
                self.state_transition(JamBLErState::DiscoveringAAs);
            }
        };
    }


    /// What happens on a user interrupt.
    /// For now, just idle.
    fn user_interrupt(&mut self) {
        self.state_transition(JamBLErState::Idle);
    }

    fn set_interval_timer(&mut self, req : IntervalTimerRequirements) {
        match req {
            IntervalTimerRequirements::NoChanges => {},
            IntervalTimerRequirements::NoIntervalTimer => {
                self.jammer_interval_timer.reset();
            },
            IntervalTimerRequirements::Countdown(interval) => {
                self.jammer_interval_timer.config(interval, false);
                self.jammer_interval_timer.start();
            },
            IntervalTimerRequirements::Periodic(interval) => {
                self.jammer_interval_timer.config(interval, true);
                self.jammer_interval_timer.start();
            }
        }
    }
    

    /// A state transition can reset the timer twice.
    /// It is first reset to prevent any new timer interrupts.
    /// The set_interval_timer will also reset the timer if it starts or reset the timer.
    /// Better safe than sorry for now.
    pub fn state_transition(&mut self, new_state : JamBLErState) {
        
        self.jammer_interval_timer.reset();
        rprintln!("Transitioning state: {:?} -> {:?}", &self.state, &new_state);
        let current_time = self.jammer_timer.get_time_micro_seconds();

        // TODO match calling stop on current state
        // stop previous state
        match self.state {
            JamBLErState::Idle => {
                let state = &mut self.state_store.idle;

                state.stop(&mut self.jammer_hal, current_time);
            },
            JamBLErState::DiscoveringAAs => {
                let state = &mut self.state_store.discover_aas;

                state.stop(&mut self.jammer_hal, current_time);
            },
        }

        let timing_requirements;
        
        
        // TODO at some point find a way around these ugly matches by making an object trait out of a JammerState. For now, first do ble stuff before code cleanup. See if it works first before all. YAGNI
        //let t : JammerState = d;
        

        // configure the state, initialise it, get its timing requirements
        // and launch it.
        match new_state {
            JamBLErState::Idle => {
                let state = &mut self.state_store.idle;
                let conf = StateConfig::new();

                state.config(conf);
                timing_requirements = state.initialise(&mut self.jammer_hal, current_time);

                state.launch(&mut self.jammer_hal, current_time);
            },
            JamBLErState::DiscoveringAAs => {
                let state = &mut self.state_store.discover_aas;
                let mut conf = StateConfig::new();
                conf.phy = Some(BlePHY::Uncoded1M);
                conf.previous_state = Some(self.state.clone());


                // this should be equivalent to the first case
                //transistion_to!(state, conf, self.jammer_hal, current_time, interval_timer, periodic, interval);


                state.config(conf);
                timing_requirements = state.initialise(&mut self.jammer_hal, current_time);
    
                state.launch(&mut self.jammer_hal, current_time);
            }
        };
        
        self.set_interval_timer(timing_requirements);

        self.state = new_state;
    }


    /// Radio interrupt received, dispatch it to the state
    pub fn handle_radio_interrupt(&mut self) -> () {
        let current_time = self.jammer_timer.get_time_micro_seconds();
        
        let (ret, timing_requirements) = match self.state {
            JamBLErState::Idle => {
                let state = &mut self.state_store.idle;
                state.handle_interrupt(&mut self.jammer_hal, current_time)},
            JamBLErState::DiscoveringAAs => {
                let state = &mut self.state_store.discover_aas;
                state.handle_interrupt(&mut self.jammer_hal, current_time)
            },
        };


        // Obey timing requirements
        self.set_interval_timer(timing_requirements);

        //TODO handle return
    }

    /// Received interval timer interrupt, dispatch it to the state.
    pub fn handle_interval_timer_interrupt(&mut self) {


        //TODO remove
        self.jammer_interval_timer.interrupt_handler();

        let current_time = self.jammer_timer.get_time_micro_seconds();
        // Dispatch it to the state
        //TODO macro
        let (ret, timing_requirements) = match self.state {
            JamBLErState::Idle => {
                let state = &mut self.state_store.idle;
                state.handle_interval_timer_interrupt(&mut self.jammer_hal, current_time)},
            JamBLErState::DiscoveringAAs => {
                let state = &mut self.state_store.discover_aas;
                state.handle_interval_timer_interrupt(&mut self.jammer_hal, current_time)
            },
        };

        // Obey timing requirements
        self.set_interval_timer(timing_requirements);

        //TODO handle return
        
    }


    /// Handler for the long term interrupt timer for when it wraps
    pub fn handle_timer_interrupt(&mut self) {
        self.jammer_timer.interrupt_handler();
    }
}

#[derive(Debug, Clone)]
pub enum JamBLErHalError {
    SetAccessAddressError,
}

#[derive(Debug, Clone)]
pub enum BlePHY {
    Uncoded1M,
    Uncoded2M,
    CodedS2,
    CodedS8,
}

/// The trait that a specific chip has to implement to be used by the jammer.
pub trait JamBLErHal {
    fn set_access_address(&mut self, aa : u32) -> Result<(), JamBLErHalError>;
}

pub trait JamBLErTimer {

    /// Starts the timer
    fn start(&mut self);

    /// Gets the duration since the start of the count in micro seconds.
    /// Micro should be accurate enough for any BLE event.
    fn get_time_micro_seconds(&mut self) -> u64;

    /// Resets the timer.
    fn reset(&mut self);


    /// Gets the drift of the timer in nanoseconds, rounded up.
    fn get_ppm(&mut self) -> u32;

    fn get_drift_percentage(&mut self) -> f64 {
        // ppm stands for parts per million, so divide by 1 million.
        self.get_ppm() as f64 / 1000000 as f64
    }

    /// Gets the maximum amount of time before overflow in seconds, rounded down.
    fn get_max_time_seconds(&mut self) -> Option<u64>;


    /// Gets the maximum amount of time before overflow in milliseconds, rounded down.
    fn get_max_time_ms(&mut self) -> Option<u64>;

    /// Will be called when an interrupt for the timer occurs.
    fn interrupt_handler(&mut self);
}

/// A timer which should generate an interrupt on its given interval.
pub trait JamBLErIntervalTimer {

    /// Sets the interval in microseconds and if the timer should function as a countdown or as a periodic timer.
    /// Returns false if the interval is too long for the timer.
    fn config(&mut self, interval : u32, periodic : bool) -> bool;

    /// Starts the timer
    fn start(&mut self);


    /// Resets the timer.
    fn reset(&mut self);

    //TODO delete
    fn interrupt_handler(&mut self);
}