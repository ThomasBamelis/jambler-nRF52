pub mod nrf52840;

/// The generic implementation of the vulnerability.
/// This is supposed to hold the BLE vulnerability code, not chip specific code.
pub struct JamBLEr<H: JamBLErHal> {
    jammer_hal : H,
    state : JamBLErState,
}

enum JamBLErState {
    Idle,
    RecoveringAA,
}

impl<H: JamBLErHal> JamBLEr<H> {
    pub fn new(jammer_hal : H) -> JamBLEr<H> {
        JamBLEr {
            jammer_hal,
            state: JamBLErState::Idle,
        }
    }

    pub fn list_aas(&mut self) -> () {
        match self.jammer_hal.set_access_address(1) {
            Ok(_) => {}
            Err(_) => {}
        };
        
    }

    /// TODO make this a trait and implement for every state struct.
    pub fn handle_radio_interrupt(&mut self) -> () {

    }
}

#[derive(Debug, Clone)]
pub enum JamBLErHalError {
    SetAccessAddressError,
}

/// The trait that a specific chip has to implement to be used by the jammer.
pub trait JamBLErHal {
    fn set_access_address(&mut self, aa : u32) -> Result<(), JamBLErHalError>;
}