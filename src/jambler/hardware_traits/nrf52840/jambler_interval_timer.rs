use hal::pac::TIMER1;
use nrf52840_hal as hal; // Embedded_hal implementation for my chip

use super::super::JamblerIntervalTimer;

use core::sync::atomic::{compiler_fence, Ordering::SeqCst};


/// A timer for countdowns and periods using timer 1 on the nrf.
///
pub struct Nrf52840IntervalTimer {
    timer_peripheral: TIMER1,
    interval: u32,
    periodic: bool,
}

impl Nrf52840IntervalTimer {
    pub fn new(timer_peripheral: TIMER1) -> Nrf52840IntervalTimer {
        Nrf52840IntervalTimer {
            timer_peripheral,
            interval: 0,
            periodic: false,
        }
    }
}

impl JamblerIntervalTimer for Nrf52840IntervalTimer {
    /// Sets the interval in microseconds and if the timer should function as a countdown or as a periodic timer.
    #[inline]
    fn config(&mut self, interval: u32, periodic: bool) -> bool {
        self.interval = interval;
        self.periodic = periodic;
        true
    }

    #[inline]
    fn interrupt_handler(&mut self) {
        //self.timer_peripheral.tasks_clear.write(|w| w.tasks_clear().set_bit());
        //self.start();
        //self.reset();

        let interval_in_ticks = self.interval;
        self.timer_peripheral.events_compare[0].reset(); // interval
                                                         //self.timer_peripheral.intenclr.modify(|_, w| w.compare0().clear());
                                                         //self.timer_peripheral.tasks_clear.write(|w| w.tasks_clear().set_bit());
                                                         //self.timer_peripheral.cc[0].write(|w| unsafe{w.cc().bits(interval_in_ticks)});
                                                         //self.timer_peripheral.intenset.modify(|_, w| w.compare0().set());
    }

    /// Starts the timer
    #[inline]
    fn start(&mut self) {
        // *** reset ***
        // Reset timer before starting.
        self.reset();

        /*
        rprintln!(
            "Starting interval timer: periodic {} and {} seconds",
            &self.periodic,
            self.interval as f64 / 1_000_000 as f64
        );
        */

        // Variable to make the code a bit less verbose.
        let interval_in_ticks = self.interval; // 1 tick corresponds to 1 microsecond
        let timer = &mut self.timer_peripheral;

        // *** config **

        compiler_fence(SeqCst);

        // Set timer mode
        timer.mode.write(|w| w.mode().timer());
        // Set 32 bit counter
        timer.bitmode.write(|w| w.bitmode()._32bit());
        // Set prescaler to 4. f_tick = 16Mhz / 2^prescaler = 1MHz
        // = 1 000 000 ticks per second.
        timer.prescaler.write(|w| unsafe { w.prescaler().bits(4) });

        // Set cc to interval.
        // We can just put the interval, because 1 tick is 1 micro second,
        // So it will compare after that
        timer.cc[0].write(|w| unsafe { w.cc().bits(interval_in_ticks) });
        // Enable interrupt for it.
        timer.intenset.modify(|_, w| w.compare0().set());

        // If periodic, set short for immediate restart
        // else, stop on match
        if self.periodic {
            // Says that when you hit the equality, clear the timer (set to 0)
            // the timer will just continue.
            timer
                .shorts
                .write(|w| w.compare0_stop().disabled().compare0_clear().enabled());
        //timer.shorts.write(|w| w.compare0_stop().enabled());
        } else {
            // Trigger stop task when you match when not periodic
            timer
                .shorts
                .write(|w| w.compare0_clear().enabled().compare0_stop().enabled());
        }

        // *** launch ***

        // Start timer by triggering task
        timer.tasks_start.write(|w| w.tasks_start().set_bit());

        compiler_fence(SeqCst);
    }

    /// Resets the timer.
    #[inline]
    fn reset(&mut self) {
        //rprintln!("Resetting interval timer");
        // Variable to make the code a bit less verbose.
        let timer = &mut self.timer_peripheral;

        // *** reset ***
        compiler_fence(SeqCst);

        // Disable interrupts of 4 CCs
        timer.intenclr.modify(|_, w| {
            w.compare0()
                .clear()
                .compare1()
                .clear()
                .compare2()
                .clear()
                .compare3()
                .clear()
        });
        // Stop the timer if it is running
        timer.tasks_stop.write(|w| w.tasks_stop().set_bit());
        // Clear the timer.
        timer.tasks_clear.write(|w| w.tasks_clear().set_bit());
        // Clear the events for our used CC registers
        timer.events_compare[0].reset(); // interval
                                         // Disable shorts
        timer
            .shorts
            .write(|w| w.compare0_stop().disabled().compare0_clear().disabled());

        compiler_fence(SeqCst);
    }
}
