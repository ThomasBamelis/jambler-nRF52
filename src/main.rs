#![no_std]
#![no_main]

use panic_halt as _; // Halts on panic. You can put a breakpoint on `rust_begin_unwind` to catch panics.
use nrf52840_hal as hal; // Embedded_hal implementation for my chip
use rtt_target::{rtt_init_print, rprintln}; // for logging to rtt


mod jamBLEr;
use jamBLEr::JamBLEr;


// This defines my rtic application, passing the nrf52840 hal to it.
// It also specifies we want access to the device specific peripherals (via ctx.devices = hal::peripherals)
#[rtic::app(device = crate::hal::pac, peripherals = true)]
const APP: () = {
    
    struct Resources {
        /// This struct holds all resources shared between tasks.
        /// As of now, I fully specify each path to better understand it all.
        dummy : u8,
        uarte : hal::uarte::Uarte<hal::pac::UARTE0>,
        uarte_timer : hal::Timer<hal::pac::TIMER0>,
        jambler : JamBLEr<jamBLEr::nrf52840::Nrf52840JamBLEr>,
    }

    /// Initialises the application using late resources.
    ///
    #[init(spawn = [software_task])]
    fn init(mut ctx: init::Context) -> init::LateResources {
        //let mcu_peripherals = ctx.devices;
        //let radio = hal::pac::UA;
        // This enables the high frequency clock as far as I am aware.
        // This is necessary for the bluetooth module to run.
        let _clocks = hal::clocks::Clocks::new(ctx.device.CLOCK).enable_ext_hfosc();
        rtt_init_print!();
        //let p0 = hal::gpio::p0::Parts::new(ctx.device.P0);
        // I can access core and device from ctx.


        rprintln!("Hello world!");

        // Configure uart according to adafruit feather board schematic
        let p0 = hal::gpio::p0::Parts::new(ctx.device.P0);

        let uarte : hal::uarte::Uarte<hal::pac::UARTE0> = hal::uarte::Uarte::new(
            ctx.device.UARTE0,
            hal::uarte::Pins {
                txd: p0.p0_25.into_push_pull_output(hal::gpio::Level::High).degrade(),
                rxd: p0.p0_24.into_floating_input().degrade(),
                cts: None,
                rts: None,
            },
            hal::uarte::Parity::EXCLUDED, // no parity bits
            hal::uarte::Baudrate::BAUD9600, // Baud Rate 9600
        );
        //hal::pac::Interrupt::RADIO
        //uarte.read_timeout(rx_buffer, timer, cycles)

        rprintln!("Created UARTE from device UARTE0");

        let uarte_timer = hal::Timer::new(ctx.device.TIMER0);
        rprintln!("Created a timer from device TIMER0, to be used for uarte");
        
        let radio : hal::pac::RADIO =  ctx.device.RADIO;
        let nrf_jambler = jamBLEr::nrf52840::Nrf52840JamBLEr {
            radio_peripheral : radio,
        };
        let jambler = JamBLEr::new(nrf_jambler);
        rprintln!("Took the radio peripheral.");


        ctx.spawn.software_task(3).unwrap();
        rprintln!("Spawned the software task. Returning from init with with the given peripherals.");

        init::LateResources {
            dummy : 6,
            uarte,
            uarte_timer,
            jambler,
        }
    }

    /// Puts the cpu to sleep (cpu clock), but leaves all system clocks and peripheral clocks on.
    /// This was used in an example using CCYNT, So I assume that one can still be used but I thought that was the cpu clock.
    #[idle]
    fn idle(_: idle::Context) -> ! {
        loop {
            cortex_m::asm::wfi();
        }
    }
    
    /// A handler for UART0
    #[task(binds = UARTE0_UART0 ,priority = 2, resources = [uarte, uarte_timer])]
    fn handle_uart(mut ctx : handle_uart::Context) {
        // This does not work properly yet I think
        let uarte : &mut hal::uarte::Uarte<hal::pac::UARTE0> = ctx.resources.uarte;
        let uarte_timer : &mut hal::Timer<hal::pac::TIMER0> = ctx.resources.uarte_timer;
        let uarte_rx_buf = &mut [0u8; 1][..];
        uarte.read_timeout( uarte_rx_buf, uarte_timer, 100_000);
        if let Ok(msg) = core::str::from_utf8(&uarte_rx_buf[..]) {
            rprintln!("Received {}", msg);
        }
        uarte.write(&uarte_rx_buf);

    }

    /// A handler for radio events
    #[task(binds = RADIO ,priority = 3, resources = [jambler, dummy])]
    fn handle_radio(mut ctx : handle_radio::Context) {
        rprintln!("Received interrupt from the radio. Should check its events.");
        let jambler : &mut JamBLEr<jamBLEr::nrf52840::Nrf52840JamBLEr> = ctx.resources.jambler;
        jambler.handle_radio_interrupt();
    }

    /// A software task receiver.
    /// It has priority 1, lower is less priority.
    /// Idle task has priority 0.
    /// Scheduling is preemptive.
    #[task(priority = 1, resources = [dummy])]
    fn software_task(mut ctx : software_task::Context, task : u8) {
        //let software_task::Resources {
        //    dummy,
        //} = ctx.resources;
        let mut dummy_copy : u8 = 0;
        // radio has access to dummy as well and this is lower prio: lock needed
        ctx.resources.dummy.lock(|dummy| {
            // data can only be modified within this critical section (closure)
            dummy_copy = *dummy;
        });

        rprintln!("Hello world from the software task!");
        if task == 3 {
            rprintln!("We received a {} from the caller as well!", task);
        }
        
        if dummy_copy == 6 as u8 {
            rprintln!("We also have access to the shared state of dummy: {}.", dummy_copy);
        }
        
    }


    // The unused interrupt used for triggering software tasks.
    // Every separate task needs its own as far as I know.
    extern "C" {
        fn SWI0_EGU0();
    }
};

