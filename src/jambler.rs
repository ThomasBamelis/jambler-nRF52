mod state;
mod util;
mod hardware_traits;
mod reversing_connection_parameters;

use reversing_connection_parameters::reverse_calculate_crc_init;

// Re-export hardware implementations for user
use heapless::Vec;
pub use hardware_traits::nrf52840;

use heapless::{consts::*, String};
use state::IntervalTimerRequirements;
use state::StateConfig;
use state::StateStore;
use state::{StateParameters, StateReturn, StateMessage};
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
    timing_delays : TimingDelays,
}

/// Holds the delays states suffer due to the framework when working with the interval timer.
/// Can be used to anticipate delays and recalculate timing requests.
struct TimingDelays {
    state_change_delay : i32, 
    periodic_no_change_delay : i32, 
    interval_timer_change_delay : i32
}

/// TODO move to state.rs
#[derive(Clone, Debug)]
pub enum JamBLErState {
    Idle,
    DiscoveringAAs,
    HarvestingPackets,
    CalibrateIntervalTimer,
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
            timing_delays : TimingDelays {
                state_change_delay : 0, 
                periodic_no_change_delay : 0, 
                interval_timer_change_delay : 0},
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
                config.phy = Some(BlePHY::Uncoded2M);
                config.slave_phy = Some(BlePHY::Uncoded2M);
                config.interval = Some(4_000_000);
                let mut cc : Vec<u8, U64> = Vec::new();
                for i in 24..=24 {
                    cc.push(i).unwrap();
                }
                config.channel_chain = Some(cc);

                config.number_of_intervals = Some(5);

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
        // TODO incorporate timing delays in this. For requesting periodics, use a countdown first incorporating the known delay and the state processing delay you measured. 
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


        rprintln!("State transition took {} micros.", self.jammer_timer.get_time_micro_seconds() - current_time);

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

        
        rprintln!("Took state {} micros to handle radio interrupt.", self.jammer_timer.get_time_micro_seconds() - current_time);

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
    /// 
    /// Because this gets called in a closure, we have to send the return via a pointer that will be filled.
    #[inline]
    pub fn handle_interval_timer_interrupt(&mut self, return_to_be_filled: &mut Option<JamBLErReturn>) {
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

        rprintln!("Took state {} micros to handle timer interrupt.", self.jammer_timer.get_time_micro_seconds() - current_time);

        // process return
        let return_filler : Option<JamBLErReturn>;
        match state_return {
            Ok(ok_state_return) => {
                match ok_state_return {
                    Some(ret) => {
                        // Obey timing requirements ASAP
                        if let Some(timing_requirements) = &ret.timing_requirements {
                            self.set_interval_timer(timing_requirements);
                        }
    
                        // Process the return value.
                        return_filler = self.process_state_return_value(ret);
                    },
                    None => {
                        return_filler = None;
                    }
                }
            },
            Err(state_error) => {
                rprintln!("ERROR: by state in interval timer interrupt\n{:?}", state_error);
                panic!()
            }
        }
        *return_to_be_filled = return_filler;
    }

    /// Handler for the long term interrupt timer for when it wraps
    #[inline]
    pub fn handle_timer_interrupt(&mut self) {
        self.jammer_timer.interrupt_handler();
    }

    /// Processes the return from a state, regardless from which interrupt.
    /// The state is telling the controller something here and should act accordingly.
    /// TODO ask for parameter for the time before and after the time the state processed the passthrough to enable for timer stuff
    /// TODO move the set interval timer here as well
    #[inline]
    fn process_state_return_value(&mut self, return_type : StateReturn) -> Option<JamBLErReturn> {
        
        let mut jambler_return = JamBLErReturn::NoReturn;

        // TODO match on state messages and do what you have to do
        // Process any state return messages
        if let Some(m) = return_type.state_message {
            match m {
                StateMessage::IntervalTimerDelays(state_change_delay, periodic_no_change_delay, interval_timer_change_delay) => {
                    rprintln!("State change delay: {} micros\nPeriodic without change delay: {} micros\nInterval timer change delay {} micros", state_change_delay, periodic_no_change_delay, interval_timer_change_delay);

                    self.timing_delays = TimingDelays {
                        state_change_delay, 
                        periodic_no_change_delay, 
                        interval_timer_change_delay};
    
                    // Tell RTIC we are done initialising
                    jambler_return = JamBLErReturn::InitialisationComplete;

                }
                // TODO delete
                StateMessage::HarvestedSubevent(mut harvested, _, wrap) => {
                    let master_pdu = &mut harvested.packet;
                    let master_crc = harvested.packet_crc;
                    let mut master_len = (master_pdu[1] as u16) + 2;
                    if master_pdu[0] & 0b0010_0000 != 0 {
                        master_len += 1;
                    }
                    let master_crc_init = reverse_calculate_crc_init(master_crc, master_pdu, master_len);
                    if let Some((slave_pdu, slave_crc, slave_rssi)) = harvested.response {
                        let mut slave_len = (slave_pdu[1] as u16) + 2;
                        if slave_pdu[0] & 0b0010_0000 != 0 {
                            slave_len += 1;
                        }
                        let slave_crc_init = reverse_calculate_crc_init(slave_crc, &slave_pdu, slave_len);


                        rprintln!("Received full connection subevent\nMaster S0, len, crc, crc_init, rssi: {:08b} {} 0x{:06X} 0x{:06X} {}\nSlave S0, len, crc, crc_init, rssi: {:08b} {} 0x{:06X} 0x{:06X} {}\n", master_pdu[0], master_pdu[1], master_crc, master_crc_init, harvested.packet_rssi, slave_pdu[0], slave_pdu[1], slave_crc, slave_crc_init, slave_rssi);

                    }
                    else {
                        rprintln!("Received only 1 pdu from connection subevent\nPacket S0, len, crc, crc_init, rssi: {:08b} {} 0x{:06X} 0x{:06X} {}\n", master_pdu[0], master_pdu[1], master_crc, master_crc_init, harvested.packet_rssi);
                    }
                }
                _ => {
                    rprintln!("State message: {:?}",m);
                }
            }
        }

        // If a state change request, do its
        if let Some((new_state, config_option)) =  return_type.state_transition {
            // If config was None, give the default empty state config
            self.state_transition(new_state, config_option.unwrap_or(StateConfig::new()))
        }

        Some(jambler_return)
    }

    /// Initialise the jambler.
    /// For now, only calibrate the interval timer.
    pub fn initialise(&mut self) {

        // The calibration interval in microseconds
        // Take this as low as you can but still larger than any possible delay
        // => figure out by trial and error
        const CALIBRATION_INTERVAL : u32 = 10_000;

        let mut config = StateConfig::new();
        config.interval = Some(CALIBRATION_INTERVAL);

        // Start with calibration
        self.state_transition(JamBLErState::CalibrateIntervalTimer, config);
    }
}


#[derive(Debug, Clone)]
pub enum JamBLErReturn {
    OutputString(String<U256>),
    InitialisationComplete,
    NoReturn,
}


#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BlePHY {
    Uncoded1M,
    Uncoded2M,
    CodedS2,
    CodedS8,
}
