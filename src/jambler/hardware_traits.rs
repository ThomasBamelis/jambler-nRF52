
pub mod nrf52840;

use super:: BlePHY;


#[derive(Debug, Clone)]
pub enum JamBLErHalError {
    SetAccessAddressError,
    InvalidChannel(u8),
}

/// The trait that a specific chip has to implement to be used by the jammer.
/// 
/// Reset can be called at any point.
/// 
/// ANY FUNCTION HERE SHOULD BE INLINED IN IMPLEMENTATION!
pub trait JamBLErHal {
    //TODO delete
    fn set_access_address(&mut self, aa: u32) -> Result<(), JamBLErHalError>;


    /// Start sending with the current configuration.
    /// Radio should be configure before this.
    /// Should be called shortly after config and fire up very fast, so any speedup achieved by making the radio more ready but consume more power should already running.
    fn send(&mut self);

    /// Start receiving with the current configuration.
    /// Radio should be configured before this.
    /// Should be called shortly after config and fire up very fast, so any speedup achieved by making the radio more ready but consume more power should already running.
    fn receive(&mut self);

    /// Should reset the radio to the same state as if at was on power on.
    /// Should be in some sort of idle or powered off state.
    /// Will often get called on initialising the radio for a new state.
    /// Should prevent any radio interrupts from happening past this point.
    fn reset(&mut self);

    /// Should prepare the radio for a configuration change.
    /// This might be a reset, but that may be too harsh.
    /// Any configurations between the previous reset and now should remain the exact same.
    /// It is more to safely change the access address for example and maybe the chip requires you should not be sending.
    fn prepare_for_config_change(&mut self);

    /// Should "pause" the radio, stopping any interrupt from being received.
    /// Should not change anything to the configuration and does not need to be a low power mode.
    fn idle(&mut self);


    /* // *** Discovering access addresses *** */

    /// Should get the radio ready for listening on the given phy and channel
    /// This config is special because many chips require hacks and cannot sniff every possible packet normally by listening.
    fn config_discover_access_addresses(&mut self, phy : BlePHY, channel : u8) -> Result<(), JamBLErHalError>;

    /// Reads the access address from the receive buffer of you chip.
    /// Might be hacky for certain chips.
    fn read_discovered_access_address(&mut self)-> Option<(u32, i8)>;
}


/// A long term timer.
/// Should be accurate up until a microseconds and last for more than the lifetime of a human (= u64 wraparound counter).
/// TODO callback for correcting for a number of microseconds (BLE slave anchor point synchronisation, clock synchronisation over I2C). 
pub trait JamBLErTimer {
    /// Starts the timer
    fn start(&mut self);

    /// Gets the duration since the start of the count in micro seconds.
    /// Micro should be accurate enough for any BLE event.
    /// SHOULD ALWAYS BE INLINED
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
/// ANY FUNCTION HERE SHOULD BE INLINED IN IMPLEMENTATION!
pub trait JamBLErIntervalTimer {
    /// Sets the interval in microseconds and if the timer should function as a countdown or as a periodic timer.
    /// Returns false if the interval is too long for the timer.
    fn config(&mut self, interval: u32, periodic: bool) -> bool;

    /// Starts the timer
    /// 
    fn start(&mut self);

    /// Resets the timer.
    fn reset(&mut self);

    /// Anything a timer needs to do to keep itself going on an interrupt.
    fn interrupt_handler(&mut self);
}
