use super::super::JamBLErHal;
use super::StateConfig;
use super::{HandlerReturn, JammerState};
use crate::jambler::state::IntervalTimerRequirements;
use crate::jambler::state::StateError;
use crate::jambler::JamBLErState;
use crate::jambler::JamBLErState::*;

pub struct Idle {}

impl JammerState for Idle {
    fn new() -> Idle {
        Idle {}
    }
    fn config(&mut self, parameters: StateConfig) -> Result<(), StateError> {
        Ok(())
    }
    fn initialise(
        &mut self,
        _radio: &mut impl JamBLErHal,
        _instant_in_microseconds: u64,
    ) -> IntervalTimerRequirements {
        IntervalTimerRequirements::NoIntervalTimer
    }

    fn launch(&mut self, radio: &mut impl JamBLErHal, instant_in_microseconds: u64) {
        // TODO put radio to sleep?
    }

    fn stop(&mut self, radio: &mut impl JamBLErHal, instant_in_microseconds: u64) {
        // TODO turn radio back on?
    }


    #[inline]
    fn handle_radio_interrupt(
        &mut self,
        _radio: &mut impl JamBLErHal,
        _instant_in_microseconds: u64,
    ) -> (HandlerReturn, IntervalTimerRequirements) {
        (
            HandlerReturn::NoReturn,
            IntervalTimerRequirements::NoChanges,
        )
    }


    #[inline]
    fn handle_interval_timer_interrupt(
        &mut self,
        _radio: &mut impl JamBLErHal,
        _instant_in_microseconds: u64,
    ) -> (HandlerReturn, IntervalTimerRequirements) {
        (
            HandlerReturn::NoReturn,
            IntervalTimerRequirements::NoChanges,
        )
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
            _ => Err(StateError::InvalidStateTransition(
                "Idle to a non-start state.",
            )),
        }
    }
}
