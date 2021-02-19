use super::StateParameters;
use super::StateReturn;
use super::super::JamBLErHal;
use super::StateConfig;
use super::{JammerState};
use crate::jambler::state::IntervalTimerRequirements;
use crate::jambler::state::StateError;
use crate::jambler::JamBLErState;
use crate::jambler::JamBLErState::*;

pub struct Idle {}

impl JammerState for Idle {

    fn new() -> Idle {
        Idle {}
    }
    fn config(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) -> Result<(), StateError> {
        Ok(())
    }
    fn initialise(
        &mut self,
        parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Result<Option<StateReturn>, StateError> {
        // Should not require an interval timer, None should not change anything (timer gets reset anyway on a state transition).
        Ok(None)
    }

    fn launch(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) {
        // TODO put radio to sleep?
    }

    fn update_state(
        &mut self, parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Result<Option<StateReturn>, StateError> {
        //Ok(None)
        // Should not be called
        panic!()
    }

    fn stop(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) {
        // TODO turn radio back on?
    }


    #[inline]
    fn handle_radio_interrupt(
        &mut self,
        parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Result<Option<StateReturn>, StateError> {
        // Should never be reached
        panic!()
    }


    #[inline]
    fn handle_interval_timer_interrupt(
        &mut self,
        parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Result<Option<StateReturn>, StateError> {
        // Should never be reached
        panic!()
    }

    /// Can transition to Idle from any state
    fn is_valid_transition_from(&mut self, old_state: &JamBLErState) -> Result<(), StateError> {
        match old_state {
            _ => Ok(()),
        }
    }

    /// Should only be ok for start states.
    fn is_valid_transition_to(&mut self, new_state: &JamBLErState) -> Result<(), StateError> {
        match new_state {
            Idle => Ok(()),
            DiscoveringAAs => Ok(()),
            JamBLErState::HarvestingPackets => Ok(()),
            _ => Err(StateError::InvalidStateTransition(
                "Idle to a non-start state.",
            )),
        }
    }
}
