use super::super::JamblerHal;
use super::JammerState;
use super::StateParameters;
use super::StateReturn;
use crate::jambler::JamblerState;

pub struct Idle {}

impl JammerState for Idle {
    fn new() -> Idle {
        Idle {}
    }

    fn config(&mut self, radio: &mut impl JamblerHal, parameters: &mut StateParameters) {}

    fn initialise(
        &mut self,
        radio: &mut impl JamblerHal,
        parameters: &mut StateParameters,
        return_value: &mut StateReturn,
    ) {
    }

    fn launch(&mut self, radio: &mut impl JamblerHal, parameters: &mut StateParameters) {
        // TODO put radio to sleep? poweroff?
    }

    fn update_state(
        &mut self,
        radio: &mut impl JamblerHal,
        parameters: &mut StateParameters,
        return_value: &mut StateReturn,
    ) {
        // Should not be called
        panic!("State update on Idle called")
    }

    fn stop(&mut self, parameters: &mut StateParameters) {
        // TODO turn radio back on?
    }

    fn handle_radio_interrupt(
        &mut self,
        radio: &mut impl JamblerHal,
        parameters: &mut StateParameters,
        return_value: &mut StateReturn,
    ) {
        // Should never be reached
        panic!("Radio interrupt on idle")
    }

    fn handle_interval_timer_interrupt(
        &mut self,
        radio: &mut impl JamblerHal,
        parameters: &mut StateParameters,
        return_value: &mut StateReturn,
    ) {
        // Should never be reached
        panic!("Interval timer interrupt on idle")
    }

    /// Can transition to Idle from any state
    fn is_valid_transition_from(&mut self, old_state: &JamblerState) {
    }

    /// Should only be ok for start states.
    #[allow(unreachable_patterns)]
    fn is_valid_transition_to(&mut self, new_state: &JamblerState) {
        match new_state {
            JamblerState::Idle
            | JamblerState::DiscoveringAAs
            | JamblerState::CalibrateIntervalTimer
            | JamblerState::HarvestingPackets => {}
            _ => panic!("Idle to a non-start state."),
        }
    }
}
