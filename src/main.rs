#![no_std]
#![no_main]

// pick a panicking behavior
use panic_halt as _; // you can put a breakpoint on `rust_begin_unwind` to catch panics
// use panic_abort as _; // requires nightly
// use panic_itm as _; // logs messages over ITM; requires ITM support
// use panic_semihosting as _; // logs messages to the host stderr; requires a debugger

#[allow(unused_imports)]
use nrf52840_hal as hal; // Always has to be important to provide vector table.

//use cortex_m::asm;
use cortex_m_rt::entry;
use rtt_target::{rtt_init_print, rprintln};
//use cortex_m_semihosting::hprintln; // semihosting to print text to the host console with hprintln. (debug deleted)

#[entry]
fn main() -> ! {
    
    rtt_init_print!();

    loop {
        // your code goes here
        rprintln!("Hello world!");
        
    }
}
