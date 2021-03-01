use super::super::JamBLErHal;
use super::JammerState;
use super::StateConfig;
use super::StateParameters;
use super::StateReturn;
use crate::jambler::state::IntervalTimerRequirements;
use crate::jambler::state::StateError;
use crate::jambler::JamBLErState;

pub struct Idle {}

impl JammerState for Idle {
    fn new() -> Idle {
        Idle {}
    }

    fn config(&mut self, radio: &mut impl JamBLErHal, parameters: &mut StateParameters) {}

    fn initialise(
        &mut self,
        radio: &mut impl JamBLErHal,
        parameters: &mut StateParameters,
        return_value: &mut StateReturn,
    ) {
    }

    fn launch(&mut self, radio: &mut impl JamBLErHal, parameters: &mut StateParameters) {
        // TODO put radio to sleep? poweroff?
    }

    fn update_state(
        &mut self,
        radio: &mut impl JamBLErHal,
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
        radio: &mut impl JamBLErHal,
        parameters: &mut StateParameters,
        return_value: &mut StateReturn,
    ) {
        // Should never be reached
        panic!("Radio interrupt on idle")
    }

    fn handle_interval_timer_interrupt(
        &mut self,
        radio: &mut impl JamBLErHal,
        parameters: &mut StateParameters,
        return_value: &mut StateReturn,
    ) {
        // Should never be reached
        panic!("Interval timer interrupt on idle")
    }

    /// Can transition to Idle from any state
    fn is_valid_transition_from(&mut self, old_state: &JamBLErState) {
        match old_state {
            _ => {
                // can come here from all states
            }
        }
    }

    /// Should only be ok for start states.
    fn is_valid_transition_to(&mut self, new_state: &JamBLErState) {
        match new_state {
            JamBLErState::Idle
            | JamBLErState::DiscoveringAAs
            | JamBLErState::CalibrateIntervalTimer
            | JamBLErState::HarvestingPackets => {}
            _ => panic!("Idle to a non-start state."),
        }
    }
}
