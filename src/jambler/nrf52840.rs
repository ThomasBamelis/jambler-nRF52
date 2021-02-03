
use nrf52840_hal as hal; // Embedded_hal implementation for my chip
use super::{JamBLErHal, JamBLErHalError, JamBLErTimer, JamBLErIntervalTimer};
use hal::pac::{RADIO, TIMER2, TIMER1, timer0, timer0::RegisterBlock as RegBlock0};

use core::sync::atomic::{compiler_fence, Ordering::SeqCst};


use rtt_target::rprintln;

/// A struct for altering the radio module of the nrf52840.
pub struct Nrf52840JamBLEr {
    pub radio_peripheral : RADIO,
}


/// Implement the necessary tools for the jammer.
impl JamBLErHal for Nrf52840JamBLEr {
    fn set_access_address(&mut self, aa : u32) -> Result<(), JamBLErHalError> {
        Ok(())
    }
}

/// A struct for using a timer on the nrf for ble.
/// Wraps around Timer 2 of the nrf52480, with a prescaler of 4 and 32-bit counter,
/// resulting in 1 microsecond accuracy, 1Mhz clock usage (low power) and ~4295 second = ~71 minute wraparound time. 
/// The timer itself has a counter which will function as a secondary level counter resulting in 71 * 2^64 wrap time. 
/// I will probably be dead before that, so ignore the wrapping time.
/// I presume the handler will be called within 71 minutes as well, so ignore that case as well.
pub struct Nrf52840Timer {
    /// Uses timer 2. This has 4 capture/compare registers.
    /// The CC[0] will be used for capturing the current seconds,
    /// while the CC[1] will be used for comparing, to throw an interrupt 
    /// and up our second level counter when the timer wraps.
    timer_peripheral : TIMER2,
    nb_times_wrapped : u32,
}


impl Nrf52840Timer {
    pub fn new(timer_peripheral : TIMER2) -> Nrf52840Timer {
        Nrf52840Timer {
            timer_peripheral,
            nb_times_wrapped : 0,
        }
    }

    // The 
    //#[inline(always)]
    //fn as_timer0(&self) -> &RegBlock0 {
    //    &self.timer_peripheral
    //}
}

impl JamBLErTimer for Nrf52840Timer {

    fn start(&mut self) {

        

        // *** reset ***

        // Reset timer before starting.
        self.reset();
        
        // Variable to make the code a bit less verbose.
        let timer = &mut self.timer_peripheral;

        // *** config **


        compiler_fence(SeqCst);

        // Set timer mode
        timer.mode.write(|w| w.mode().timer());
        // Set 32 bit counter
        timer.bitmode.write(|w| w.bitmode()._32bit());
        // Set prescaler to 4. f_tick = 16Mhz / 2^prescaler = 1MHz
        // = 1 000 000 ticks per second.
        // This results in 2^32 / 1 000 000 seconds before overflow of 32-bit counter.
        timer.prescaler.write(|w| unsafe {w.prescaler().bits(4)});

        // Set overflow compare register to when it would be filled with 1s.
        //TODO reset to 0xFFFFFFFF 0x000F4240
        timer.cc[1].write(|w| unsafe{w.cc().bits(0xFFFFFFFF)});
        // Enable interrupt for it.
        timer.intenset.modify(|_, w| w.compare1().set());

        // *** launch ***

        // Start timer by triggering task
        timer.tasks_start.write(|w| w.tasks_start().set_bit());

        compiler_fence(SeqCst);

    }

    /// Gets the duration since the start of the count in micro seconds.
    /// Micro should be accurate enough for any BLE event.
    fn get_time_micro_seconds(&mut self) -> u64 {


        compiler_fence(SeqCst);
        self.timer_peripheral.tasks_capture[0].write(|w| w.tasks_capture().set_bit());
        compiler_fence(SeqCst);
        let current_ticks : u32 = self.timer_peripheral.cc[0].read().bits();
        compiler_fence(SeqCst);

        //rprintln!("Got long term timer current time, register contents: {}", current_ticks);

        // calculate to total amount of ticks
        // 1 000 000 ticks per second
        let ticks_per_micro_second : u64 = 1;
        //TODO reset to 0xFFFFFFFF 0x000F4240
        let ms_from_wrap_around : u64 = self.nb_times_wrapped as u64 * 0xFFFFFFFF as u64 * ticks_per_micro_second; // u32 * u32 * 1 should fit in u64
        let ms_from_this_cycle : u64 = current_ticks as u64 * ticks_per_micro_second;

        ms_from_this_cycle + ms_from_wrap_around // will overflow when I am dead
    }

    /// Resets the timer. The timer is stopped after this.
    fn reset(&mut self) {
        // Variable to make the code a bit less verbose.
        let timer = &mut self.timer_peripheral;

        // *** reset ***


        compiler_fence(SeqCst);

        // Disable interrupts of 4 CCs
        timer.intenclr.modify(|_, w| w.compare0().clear().compare1().clear().compare2().clear().compare3().clear());
        // Stop the timer if it is running
        timer.tasks_stop.write(|w| w.tasks_stop().set_bit());
        // Clear the timer.
        timer.tasks_clear.write(|w| w.tasks_clear().set_bit());
        // Clear the events for our used CC registers
        timer.events_compare[0].reset(); // capture
        timer.events_compare[1].reset(); // wraparound compare


        compiler_fence(SeqCst);
    }


    /// Gets the accuracy of the timer in ppm.
    fn get_ppm(&mut self) -> u32 {
        // datasheet: Independent of prescaler setting the accuracy of the TIMER is equivalent to one tick of the timerfrequency fTIMER
        // = 1 second / 1 000 000 = 1 micro second = 1000 nano seconds
        // page 96 of datasheet, for 32M clock which will drive the 1M signal
        60
    }

    /// Gets the maximum amount of time before overflow in seconds, rounded down.
    /// None means it cannot be expressed in a u64.
    fn get_max_time_seconds(&mut self) -> Option<u64> {
        None
    }


    /// Gets the maximum amount of time before overflow in milliseconds, rounded down.
    /// None means it cannot be expressed in a u64.
    fn get_max_time_ms(&mut self) -> Option<u64> {
        None
    }

    /// Will be called when an interrupt for the timer occurs.
    fn interrupt_handler(&mut self) {
        

        compiler_fence(SeqCst);
        let cc1_event : bool = self.timer_peripheral.events_compare[1].read().bits() != 0;
        
        if cc1_event {
            // increment the second level timer
            self.nb_times_wrapped += 1;
            // reset event
            self.timer_peripheral.events_compare[1].reset(); 

            //TODO delete
            //self.timer_peripheral.tasks_clear.write(|w| w.tasks_clear().set_bit());
        }

        compiler_fence(SeqCst);
    }

}





/// A timer for countdowns and periods using timer 1 on the nrf.
/// 
pub struct Nrf52840IntervalTimer {
    timer_peripheral : TIMER1,
    interval : u32,
    periodic : bool,
}


impl Nrf52840IntervalTimer {
    pub fn new(timer_peripheral : TIMER1) -> Nrf52840IntervalTimer {
        Nrf52840IntervalTimer {
            timer_peripheral,
            interval : 0,
            periodic : false,
        }
    }
}

impl JamBLErIntervalTimer for Nrf52840IntervalTimer {

    /// Sets the interval in microseconds and if the timer should function as a countdown or as a periodic timer.
    fn config(&mut self, interval : u32, periodic : bool) -> bool {
        self.interval = interval;
        self.periodic = periodic;
        true
    }

    //TODO delete
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
    fn start(&mut self) {
        // *** reset ***
        // Reset timer before starting.
        self.reset();


        rprintln!("Starting interval timer: periodic {} and {} seconds", &self.periodic, self.interval as f64 / 1_000_000 as f64);

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
        timer.prescaler.write(|w| unsafe {w.prescaler().bits(4)});

        // Set cc to interval.
        // We can just put the interval, because 1 tick is 1 micro second,
        // So it will compare after that
        timer.cc[0].write(|w| unsafe{w.cc().bits(interval_in_ticks)});
        // Enable interrupt for it.
        timer.intenset.modify(|_, w| w.compare0().set());

        // If periodic, set short for immediate restart
        // else, stop on match
        if self.periodic {
            // Says that when you hit the equality, clear the timer (set to 0)
            // the timer will just continue.
            timer.shorts.write(|w| w.compare0_stop().disabled().compare0_clear().enabled());
            //timer.shorts.write(|w| w.compare0_stop().enabled());

        }
        else {
            // Trigger stop task when you match when not periodic
            timer.shorts.write(|w| w.compare0_clear().enabled().compare0_stop().enabled());
        }

        // *** launch ***

        // Start timer by triggering task
        timer.tasks_start.write(|w| w.tasks_start().set_bit());


        compiler_fence(SeqCst);

    }


    /// Resets the timer.
    fn reset(&mut self) {

        rprintln!("Resetting interval timer");
        // Variable to make the code a bit less verbose.
        let timer = &mut self.timer_peripheral;

        // *** reset ***
        compiler_fence(SeqCst);

        // Disable interrupts of 4 CCs
        timer.intenclr.modify(|_, w| w.compare0().clear().compare1().clear().compare2().clear().compare3().clear());
        // Stop the timer if it is running
        timer.tasks_stop.write(|w| w.tasks_stop().set_bit());
        // Clear the timer.
        timer.tasks_clear.write(|w| w.tasks_clear().set_bit());
        // Clear the events for our used CC registers
        timer.events_compare[0].reset(); // interval
        // Disable shorts
        timer.shorts.write(|w| w.compare0_stop().disabled().compare0_clear().disabled());
    
        compiler_fence(SeqCst);
    }
}