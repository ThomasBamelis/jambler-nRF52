#![no_std]
#![no_main]

use panic_halt as _; // Halts on panic. You can put a breakpoint on `rust_begin_unwind` to catch panics.
use nrf52840_hal as hal; // Embedded_hal implementation for my chip
use rtt_target::{rtt_init_print, rprintln}; // for logging to rtt


mod jambler;
use jambler::JamBLEr;
use jambler::nrf52840::Nrf52840JamBLEr;

mod serial;
use serial::SerialController;
use heapless::{String, consts::*};


// This defines my rtic application, passing the nrf52840 hal to it.
// It also specifies we want access to the device specific peripherals (via ctx.devices = hal::peripherals)
#[rtic::app(device = crate::hal::pac, peripherals = true)]
const APP: () = {
    
    struct Resources {
        /// This struct holds all resources shared between tasks.
        /// As of now, I fully specify each path to better understand it all.
        dummy : u8,
        uarte : SerialController,
        jambler : JamBLEr<Nrf52840JamBLEr>,
    }

    /// Initialises the application using late resources.
    ///
    #[init(spawn = [software_task])]
    fn init(ctx: init::Context) -> init::LateResources {
        
        // This enables the high frequency clock as far as I am aware.
        // This is necessary for the bluetooth and uart module to run.
        let _clocks = hal::clocks::Clocks::new(ctx.device.CLOCK).enable_ext_hfosc();

        // Init rtt for debugging
        rtt_init_print!();
        rprintln!("Booting up.");

        // setup uart
        // Configure uart according to adafruit feather board schematic
        // I had to hardcode the ports in the serial controller.
        let p0 = hal::gpio::p0::Parts::new(ctx.device.P0);
        let uart_pins = hal::uarte::Pins {
            txd: p0.p0_25.into_push_pull_output(hal::gpio::Level::High).degrade(),
            rxd: p0.p0_24.into_floating_input().degrade(),
            cts: None,
            rts: None,
        };
        let uart_device : hal::pac::UARTE1 = ctx.device.UARTE1;
        let mut uarte = SerialController::new(uart_device, uart_pins);
        //rprintln!("Created UARTE from device UARTE1.");
        

        // setup jammer
        let radio : hal::pac::RADIO =  ctx.device.RADIO;
        let nrf_jambler = Nrf52840JamBLEr {
            radio_peripheral : radio,
        };
        let jambler = JamBLEr::new(nrf_jambler);
        //rprintln!("Initialised jammer.");

        // Spawn the software task.
        ctx.spawn.software_task(3).unwrap();
        //rprintln!("Spawned the software task.");

        // Start listening
        uarte.start_listening();
        //rprintln!("Started listening on uart.");

        
        let mut welcome : String<U256> = String::new();
        welcome.push_str("\r\n#################\
                          \r\n#  \\|/          #\
                          \r\n# --o-- JamBLEr #\
                          \r\n#  /|\\          #\
                          \r\n#################\r\n").unwrap();
        uarte.send_string(welcome);
        welcome = String::new();
        welcome.push_str("\r\nWelcome my friend!\r\nPress backtick ` to interrupt at any point during execution.\r\nBackspace is supported but will not remove written characters from your screen.\r\nReset button works.\r\nType a command and press enter:\r\n").unwrap();
        uarte.send_string(welcome);

        //rprintln!("Welcome message sent.");


        rprintln!("Initialisation complete.");
        init::LateResources {
            dummy : 6,
            uarte,
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
    
    

    /// A handler for radio events
    #[task(binds = RADIO ,priority = 7, resources = [jambler, dummy])]
    fn handle_radio(ctx : handle_radio::Context) {
        rprintln!("Received interrupt from the radio. Should check its events.");
        let jambler : &mut JamBLEr<Nrf52840JamBLEr> = ctx.resources.jambler;
        jambler.handle_radio_interrupt();
    }

    /// A handler for UART1
    /// UART1 is only available on the nrf52840, sorry not sorry
    #[task(binds = UARTE1 ,priority = 6, resources = [uarte])]
    fn handle_uart(ctx : handle_uart::Context) {
        // get the resource
        let uarte : &mut SerialController = ctx.resources.uarte;
        
        if let Some(mut task) = uarte.handle_interrupt() {
            // TODO dispatch task

            // for now, echo
            rprintln!("Received task over uart: {}", &task);
            
            task.push_str("\r\n").unwrap();
            uarte.send_string(task);
            // after first interrupt with `, start listening for others.
            uarte.init_receive_string();
        }

    }

    /// A software task receiver.
    /// It has priority 1, lower is less priority.
    /// Idle task has priority 0.
    /// Scheduling is preemptive.
    #[task(priority = 1, resources = [dummy, uarte])]
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


        ctx.resources.uarte.lock(|uarte| {
            // data can only be modified within this critical section (closure)
            let dev : &mut SerialController = uarte;
            let mut welcome : String<U256> = String::new();
            /*
            welcome.push_str("\r\n#################\
                              \r\n#  \\|/          #\
                              \r\n# --o-- JamBLEr #\
                              \r\n#  /|\\          #\
                              \r\n#################\r\n").unwrap();
            welcome.push_str("Welcome my friend!\r\nPress backtick ` to interrupt at any point during execution.\r\nBackspace is supported but will not remove written characters from your screen.\r\nType a command and press enter:\r\n").unwrap();
            dev.send_string(welcome);
            welcome = String::new();
            */
            welcome.push_str("\r\nThis build will echo you commands after sending a first interrupt.\r\nSome unknown bug causes the first character you send to not be received, so send a dummy one as well first.\r\n").unwrap();
            dev.send_string(welcome);
            welcome = String::new();
            welcome.push_str("However, I think it is a problem with minicom not sending raw information\r\nRight now when you press enter it sends linefeed instead of new line\r\nThe first received character is always \\0, which might be another minicom thing maybe?\r\n").unwrap();
            dev.send_string(welcome);
            rprintln!("Sent welcome string from software task.");
        });
        
    }


    // The unused interrupt used for triggering software tasks.
    // Every separate task needs its own as far as I know.
    extern "C" {
        fn SWI0_EGU0();
    }
};

