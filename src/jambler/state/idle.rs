use crate::jambler::state::IntervalTimerRequirements;
use super::StateConfig;
use super::{JammerState, HandlerReturn};
use super::super::{JamBLErHal};


pub struct Idle {

}

impl JammerState for Idle {

    fn new() -> Idle {Idle {}}
    fn config(&mut self, parameters : StateConfig) -> bool {true}
    fn initialise(&mut self, _radio : &mut impl JamBLErHal, _instant_in_microseconds : u64) -> IntervalTimerRequirements {
        IntervalTimerRequirements::NoIntervalTimer
    }

    fn launch(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64) {
        // TODO put radio to sleep?
    }

    fn stop(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64) {
        // TODO turn radio back on?
    }

    fn handle_interrupt(&mut self, _radio : &mut impl JamBLErHal, _instant_in_microseconds : u64) -> (HandlerReturn, IntervalTimerRequirements)  {
        (HandlerReturn::NoReturn, IntervalTimerRequirements::NoChanges)
    }
    fn handle_interval_timer_interrupt(&mut self, _radio : &mut impl JamBLErHal, _instant_in_microseconds : u64) -> (HandlerReturn, IntervalTimerRequirements)  {
        (HandlerReturn::NoReturn, IntervalTimerRequirements::NoChanges)
    }
}