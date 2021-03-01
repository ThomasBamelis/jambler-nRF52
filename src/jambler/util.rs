use rtt_target::{rprint, rprintln};

/// A struct for keeping timestamps.
/// Used for debugging and feedback in readable format.
pub struct TimeStamp {
    days: u32,
    hours: u8,
    minutes: u8,
    seconds: u8,
    milliseconds: u16,
    microseconds: u16,
    original_micro_seconds: u64,
}

impl TimeStamp {
    pub fn from_microseconds(microseconds: u64) -> TimeStamp {
        TimeStamp {
            days: (microseconds / (24 * 60 * 60 * 1000_000)) as u32,
            hours: ((microseconds / (60 * 60 * 1000_000)) % 24) as u8,
            minutes: ((microseconds / (60 * 1000_000)) % 60) as u8,
            seconds: ((microseconds / 1_000_000) % 60) as u8,
            milliseconds: ((microseconds / 1000) % 1000) as u16,
            microseconds: (microseconds % 1000) as u16,
            original_micro_seconds: microseconds,
        }
    }

    pub fn rprint_normal(&self) {
        if self.days != 0 {
            rprint!(
                "d {} {}:{} s {} ms {}: ",
                self.days,
                self.hours,
                self.minutes,
                self.seconds,
                self.milliseconds
            );
        } else if self.hours != 0 {
            rprint!(
                "{}:{} s {} ms {}: ",
                self.hours,
                self.minutes,
                self.seconds,
                self.milliseconds
            );
        } else if self.minutes != 0 {
            rprint!(
                "min {} s {} ms {}: ",
                self.minutes,
                self.seconds,
                self.milliseconds
            );
        } else if self.seconds != 0 {
            rprint!("s {} ms {}: ", self.seconds, self.milliseconds);
        } else if self.milliseconds != 0 {
            rprint!("ms {}: ", self.milliseconds);
        }
    }

    pub fn rprint_normal_from_microseconds(microseconds: u64) {
        TimeStamp::from_microseconds(microseconds).rprint_normal();
    }

    pub fn rprint_normal_with_micros(&self) {
        if self.days != 0 {
            rprint!(
                "d {} {}:{} s {} ms {} micros {}: ",
                self.days,
                self.hours,
                self.minutes,
                self.seconds,
                self.milliseconds,
                self.microseconds
            );
        } else if self.hours != 0 {
            rprint!(
                "{}:{} s {} ms {} micros {}: ",
                self.hours,
                self.minutes,
                self.seconds,
                self.milliseconds,
                self.microseconds
            );
        } else if self.minutes != 0 {
            rprint!(
                "min {} s {} ms {} micros {}: ",
                self.minutes,
                self.seconds,
                self.milliseconds,
                self.microseconds
            );
        } else if self.seconds != 0 {
            rprint!(
                "s {} ms {} micros {}: ",
                self.seconds,
                self.milliseconds,
                self.microseconds
            );
        } else if self.milliseconds != 0 {
            rprint!("ms {} micros {}: ", self.milliseconds, self.microseconds);
        } else {
            rprint!("micros {}: ", self.microseconds);
        }
    }

    pub fn rprint_normal_with_micros_from_microseconds(microseconds: u64) {
        TimeStamp::from_microseconds(microseconds).rprint_normal_with_micros();
    }

    pub fn rprintln_normal_with_micros(&self) {
        rprintln!("");
        self.rprint_normal_with_micros();
        rprintln!("");
    }

    pub fn rprintln_normal_with_micros_from_microseconds(microseconds: u64) {
        TimeStamp::from_microseconds(microseconds).rprintln_normal_with_micros();
    }

    pub fn rprintln_normal(&self) {
        self.rprint_normal();
        rprintln!("");
    }

    pub fn rprintln_normal_from_microseconds(microseconds: u64) {
        TimeStamp::from_microseconds(microseconds).rprintln_normal();
    }

    pub fn rprint_precise(&self) {
        rprint!(
            "s {} ms {} micros {}: ",
            self.original_micro_seconds / 1_000_000,
            self.milliseconds,
            self.microseconds
        );
    }

    pub fn rprint_precise_from_microseconds(microseconds: u64) {
        TimeStamp::from_microseconds(microseconds).rprint_precise();
    }

    pub fn rprintln_precise(&self) {
        self.rprint_precise();
        rprintln!("");
    }

    pub fn rprintln_precise_from_microseconds(microseconds: u64) {
        TimeStamp::from_microseconds(microseconds).rprintln_precise();
    }

    pub fn rprint_precise_difference(left: u64, right: u64) {
        let smallest;
        let biggest;
        if left >= right {
            smallest = right;
            biggest = left;
        } else {
            smallest = left;
            biggest = right;
        }

        TimeStamp::rprint_precise_from_microseconds(biggest - smallest);
    }
}


impl core::fmt::Display for TimeStamp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.days != 0 {
            write!(f, 
                "d {} {}:{} s {} ms {} micros {}: ",
                self.days,
                self.hours,
                self.minutes,
                self.seconds,
                self.milliseconds,
                self.microseconds
            )
        } else if self.hours != 0 {
            write!(f, 
                "{}:{} s {} ms {} micros {}: ",
                self.hours,
                self.minutes,
                self.seconds,
                self.milliseconds,
                self.microseconds
            )
        } else if self.minutes != 0 {
            write!(f, 
                "min {} s {} ms {} micros {}: ",
                self.minutes,
                self.seconds,
                self.milliseconds,
                self.microseconds
            )
        } else if self.seconds != 0 {
            write!(f, 
                "s {} ms {} micros {}: ",
                self.seconds,
                self.milliseconds,
                self.microseconds
            )
        } else if self.milliseconds != 0 {
            write!(f, "ms {} micros {}: ", self.milliseconds, self.microseconds)
        } else {
            write!(f, "micros {}: ", self.microseconds)
        }
    }
}

impl core::fmt::Debug for TimeStamp {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}", self.original_micro_seconds)
    }
}