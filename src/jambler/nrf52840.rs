
use nrf52840_hal as hal; // Embedded_hal implementation for my chip
use super::{JamBLErHal, JamBLErHalError};

/// A struct for altering the radio module of the nrf52840.
pub struct Nrf52840JamBLEr {
    pub radio_peripheral : hal::pac::RADIO,
}


/// Implement the necessary tools for the jammer.
impl JamBLErHal for Nrf52840JamBLEr {
    fn set_access_address(&mut self, aa : u32) -> Result<(), JamBLErHalError> {
        Ok(())
    }
}