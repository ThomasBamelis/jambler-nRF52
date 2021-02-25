#![no_std]
#![no_main]

use crate::jambler::JamBLErReturn;
use nrf52840_hal as hal; // Embedded_hal implementation for my chip
use panic_halt as _; // Halts on panic. You can put a breakpoint on `rust_begin_unwind` to catch panics.
// TODO change panic behaviour to turn on led on board, so we can spot it with multiple leds
use rtt_target::{rprintln, rtt_init_print}; // for logging to rtt

mod jambler;
use jambler::nrf52840::{Nrf52840IntervalTimer, Nrf52840JamBLEr, Nrf52840Timer};
use jambler::{JamBLEr, JamBLErTask};

mod serial;
use heapless::{consts::*, String};
use serial::SerialController;

// This defines my rtic application, passing the nrf52840 hal to it.
// It also specifies we want access to the device specific peripherals (via ctx.devices = hal::peripherals)
#[rtic::app(device = crate::hal::pac, peripherals = true)]
const APP: () = {
    struct Resources {
        /// This struct holds all resources shared between tasks.
        /// As of now, I fully specify each path to better understand it all.
        dummy: u8,
        uarte: SerialController,
        jambler: JamBLEr<Nrf52840JamBLEr, Nrf52840Timer, Nrf52840IntervalTimer>,
    }

    /// Initialises the application using late resources.
    ///
    /// This is primarily to grab ownership of the resources and turn them into my structs.
    /// But the variables will be copied (moved) in to their static places only after this function exits.
    /// Any real initialisation should be done in the initialise_late_resources task which gets spawned exactly once right after the init function.
    #[init(spawn = [initialise_late_resources])]
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
            txd: p0
                .p0_25
                .into_push_pull_output(hal::gpio::Level::High)
                .degrade(),
            rxd: p0.p0_24.into_floating_input().degrade(),
            cts: None,
            rts: None,
        };
        let uart_device: hal::pac::UARTE1 = ctx.device.UARTE1;
        let uarte = SerialController::new(uart_device, uart_pins);

        // setup jammer
        let radio: hal::pac::RADIO = ctx.device.RADIO;
        let nrf_jambler = Nrf52840JamBLEr::new(radio);
        let timer_per: hal::pac::TIMER2 = ctx.device.TIMER2;
        let nrf_timer = Nrf52840Timer::new(timer_per);
        let interval_timer_per: hal::pac::TIMER1 = ctx.device.TIMER1;
        let interval_nrf_timer = Nrf52840IntervalTimer::new(interval_timer_per);
        
        let jambler = JamBLEr::new(nrf_jambler, nrf_timer, interval_nrf_timer);

        // Spawn the late resources initialiser, so any initialisation for which the resources must be in their final memory place can be done there.
        ctx.spawn.initialise_late_resources(InitialisationSequence::InitialiseJambler).ok();

        init::LateResources {
            dummy: 6,
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

    // TODO you can only have 7 priorities for the best performance because the nrf52840 has 3bit arm interrupt priority bits (and 0 is for idle task)
    /*
    Tasks can have priorities in the range 1..=(1 << NVIC_PRIO_BITS) where NVIC_PRIO_BITS is a constant defined in the device crate.
    The idle task has a non-configurable static priority of 0, the lowest priority. Tasks can have the same priority.
    Tasks with the same priority do not preempt each other.
    */




    /*********************************************************/
    /* // ***          HARDWARE INTERRUPT TASKS          *** */
    /*********************************************************/




    /// A handler for radio interrupts.
    /// Is passed completely to jambler.
    #[task(binds = RADIO ,priority = 7, resources = [jambler], spawn = [RTIC_controller])]
    fn handle_radio(ctx: handle_radio::Context) {
        // Interpret the resource (compiler comfort)
        let jambler: &mut JamBLEr<Nrf52840JamBLEr, Nrf52840Timer, Nrf52840IntervalTimer> = ctx.resources.jambler;

        // Pass the interrupt to jambler
        let return_instruction = jambler.handle_radio_interrupt();
        
        // If the RTIC controller needs to take action, spawn the controller with the given action.
        let rtic_controller_action = process_jambler_return(return_instruction);
        if let Some(rtic_controller_action) = rtic_controller_action {
            ctx.spawn.RTIC_controller(rtic_controller_action).unwrap();
        }
    }

    /// Handles interrupts of the INTERVAL timer used by the jammer
    #[task(binds = TIMER1 ,priority = 6, resources = [jambler], spawn = [RTIC_controller])]
    fn handle_timer1(mut ctx: handle_timer1::Context) {


        let mut return_instruction = None;

        // Get lock on the jammer to be able to call its timer interrupt.
        ctx.resources.jambler.lock(|jambler| {
            // Pass the interrupt to jambler
            jambler.handle_interval_timer_interrupt(&mut return_instruction);
        
            
        });

        // If the RTIC controller needs to take action, spawn the controller with the given action.
        let rtic_controller_action = process_jambler_return(return_instruction);
        if let Some(rtic_controller_action) = rtic_controller_action {
            ctx.spawn.RTIC_controller(rtic_controller_action).unwrap();
        }
    }

    /// A handler for UART1
    /// UART1 is only available on the nrf52840, sorry not sorry
    #[task(binds = UARTE1 ,priority = 5, resources = [uarte], spawn = [cli_command_dispatcher])]
    fn handle_uart(mut ctx: handle_uart::Context) {
        // get the resource
        let uarte: &mut SerialController = ctx.resources.uarte;

        if let Some(mut cli_command) = uarte.handle_interrupt() {
            ctx.spawn.cli_command_dispatcher(cli_command).unwrap();
        }
    }

    /// Handles interrupts of the timer used by the jammer.
    /// This is the timer used for long term timing, basically time keeping the system.
    #[task(binds = TIMER2 ,priority = 4, resources = [jambler])]
    fn handle_timer2(mut ctx: handle_timer2::Context) {
        // Get lock on the jammer to be able to call its timer interrupt.
        //rprintln!("Received interrupt from the long term timer.");
        ctx.resources.jambler.lock(|jambler| {
            jambler.handle_timer_interrupt();
        });
    }




    /***********************************************/
    /* // ***          SOFTWARE TASKS          *** */
    /***********************************************/



    /// The central controller.
    /// 
    /// Watch out, spawning a task with a higher priority will preempt the current one.
    /// Other tasks can pass requests to this task.
    /// 
    /// The responsibility of this task is to be a central point to avoid code duplication.
    #[task(priority = 2, resources = [jambler, uarte], spawn = [background_worker, initialise_late_resources])]
    fn RTIC_controller(mut ctx: RTIC_controller::Context, rtic_controller_action : RTICControllerAction) {
        match rtic_controller_action {
            RTICControllerAction::NextInitialisationStep(next_step) => {
                ctx.spawn.initialise_late_resources(next_step).unwrap();
            }
        }
    }


    /// Initialise the late resources after they have been copied into their static place.
    /// 
    /// ANY INITIALISING THAT HAS TO BE DONE BY GIVING A POINTER TO A BUFFER IN A LATER RESOURCE HAS TO BE DONE HERE AND NOT IN INIT!
    /// 
    /// Has same priority as command dispatcher for now. 
    /// Interrupts are already enabled here for the processor.
    #[task(priority = 2, resources = [jambler, uarte])]
    fn initialise_late_resources(mut ctx: initialise_late_resources::Context, point_in_init : InitialisationSequence) {


        match point_in_init {
            InitialisationSequence::InitialiseJambler => {
                // Initialise the jambler first
                ctx.resources.jambler.lock(|jambler| {
                    // 
                    jambler.initialise();
                });

                ctx.resources.uarte.lock(|uarte| {
                    // Print the welcome message
                    print_welcome_message(uarte);
                });
            }
            InitialisationSequence::InitialiseUart => {

                ctx.resources.uarte.lock(|uarte| {
                    // Start listening. Initially this will only do something when the interrupt is received.
                    uarte.start_listening();
                    // Start listening for a command
                    uarte.init_receive_string();
                    // print bootup complete message
                    print_bootup_complete_message(uarte);
                });
            }
        }
    }

    /*
    
                    RTICCommand::NextInitialisationStep(next_step) => {
                        ctx.spawn.initialise_late_resources(next_step).unwrap();
                    }
    */

    /// Will parse the commands received by uart.
    /// Separate function because this processing should not be done in an interrupt handler task.
    #[task(priority = 2, resources = [jambler, uarte], spawn = [background_worker, initialise_late_resources])]
    fn cli_command_dispatcher(mut ctx: cli_command_dispatcher::Context, command: String<U256>) {
        match parse_command(command) {
            Some(cli_command) => {
                match cli_command {
                    CLICommand::UserInterrupt => {
                        // TODO configure what happens on user interrupt
                        ctx.resources.jambler.lock(|jambler| {
                            jambler.execute_task(JamBLErTask::UserInterrupt);
                        });

                        // start listening for next command
                        ctx.resources.uarte.lock(|uarte| {
                            // data can only be modified within this critical section (closure)
                            let dev: &mut SerialController = uarte;
                            let mut error_string = String::new();
                            error_string
                                .push_str("Interrupt received.\r\nGive a new command.\r\n")
                                .unwrap();
                            dev.send_string(error_string);
                            dev.init_receive_string();
                        });
                    }
                    CLICommand::JamBLErTask(jambler_task) => {
                        // propagate jambler command to jambler
                        ctx.resources.jambler.lock(|jambler| {
                            jambler.execute_task(jambler_task);
                        });
                    }
                    CLICommand::BackgroudTask(backgound_param) => {
                        // Spawn the software task.

                        // TODO spawn call throws error if task not finished and all its static capacity is used (the size of its queue)
                        ctx.spawn.background_worker(backgound_param).unwrap();
                    }
                    _ => {
                        // Unexpected command parsed
                        panic!()
                    }
                }
            }
            None => {
                // Invalid command, print invalid command
                ctx.resources.uarte.lock(|uarte| {
                    // data can only be modified within this critical section (closure)
                    let dev: &mut SerialController = uarte;
                    let mut error_string = String::new();
                    error_string
                        .push_str("^ is an invalid command.\r\nGive a new command\r\n")
                        .unwrap();
                    dev.send_string(error_string);
                    dev.init_receive_string();
                });
            }
        }
    }

    /// A software task receiver.
    /// It has priority 1, lower is less priority.
    /// Idle task has priority 0.
    /// Scheduling is preemptive.
    #[task(priority = 1, resources = [dummy])]
    fn background_worker(mut ctx: background_worker::Context, task: u8) {
        // TODO background work = pattern match
        // TODO put messages in resource and have boolean resource flag
        // Get a lock on the flag every 1000 or so iterations to see if there are new messages, if so interrupt current work
        // Will be messy to use with a jambler function, but maybe you can pass a
        // volatile reference to the function? -> No, lock needed...
        // Maybe make jambler function able to run for x iterations from start position x and return if it found one or multiple or none in this slice.

        rprintln!("Background task called with task {}", task);
        
    }

    // The unused interrupt used for triggering software tasks.
    // Every separate task needs its own as far as I know.
    extern "C" {
        fn SWI0_EGU0();
        fn SWI1_EGU1();
        fn SWI2_EGU2();
        fn SWI3_EGU3();
    }
};





/***********************************************************************/
/* // ***          RTIC CONTROL AND DEFINITIONS FUNCTIONS          *** */
/***********************************************************************/



/// To keep track of the initialisation sequence
/// The sequence should be done in the same order as defined here
#[derive(Debug)]
enum InitialisationSequence {
    InitialiseJambler,
    InitialiseUart,
}



/// An enum for letting any task send a message to the controller for changing things.
#[derive(Debug)]
enum RTICControllerAction {
    NextInitialisationStep(InitialisationSequence),
}






/// Process jambler return values
#[inline]
fn process_jambler_return(jambler_return : Option<JamBLErReturn>) -> Option<RTICControllerAction> {
    if let Some(jr) = jambler_return {
        match jr {
            // If jambler reports his initialisation complete, move on to initialising uart
            JamBLErReturn::InitialisationComplete => {
                return Some(RTICControllerAction::NextInitialisationStep(InitialisationSequence::InitialiseUart));
            }
            _ => {}
        }
    }
    None
}




/**********************************************************************/
/* // ***          UART PROCESSING AND UTILITY FUNCTIONS          *** */
/**********************************************************************/

// RTICCommand
#[derive(Debug)]
enum CLICommand {
    JamBLErTask(JamBLErTask),
    BackgroudTask(u8),
    UserInterrupt,
}




fn print_welcome_message(uarte : &mut SerialController) {

    let mut welcome: String<U256> = String::new();
    welcome
        .push_str(
            "\r\n#################\
                      \r\n#  \\|/          #\
                      \r\n# --o-- JamBLEr #\
                      \r\n#  /|\\          #\
                      \r\n#################\r\n",
        )
        .unwrap();
    uarte.send_string(welcome);
    welcome = String::new();
    welcome.push_str("\r\nWelcome my friend!\r\nPress backtick ` to interrupt at any point during execution.\r\nBackspace is supported but will not remove written characters from your screen.\r\n").unwrap();
    uarte.send_string(welcome);
}

fn print_bootup_complete_message(uarte : &mut SerialController) {

    let mut welcome: String<U256> = String::new();
    welcome.push_str("\r\nInitialisation done.\r\nType a command and press enter:\r\n").unwrap();
    uarte.send_string(welcome);
}

/// Helper function for parsing a uart string into a command.
/// Returns Some if the command had a valid syntax.
/// The command parameters might still be invalid though.
#[inline]
fn parse_command(command: String<U256>) -> Option<CLICommand> {
    if let Some(rtic_command) = get_split(command.as_str(), ' ', 0) {
        match rtic_command {
            "INTERRUPT" => Some(CLICommand::UserInterrupt),
            "discoveraas" => Some(CLICommand::JamBLErTask(JamBLErTask::DiscoverAas)),
            "jam" => {
                if let Some(param_1_aa) = get_split(command.as_str(), ' ', 1) {
                    if let Some(u32_value_aa) = hex_str_to_u32(param_1_aa) {
                        rprintln!("Received jam command for u32 addres {}", u32_value_aa);
                        //TODO change to proper command
                        Some(CLICommand::JamBLErTask(JamBLErTask::Jam))
                    } else {
                        // 1st param was not hex value
                        None
                    }
                } else {
                    None
                }
            }
            "background" => {
                if let Some(param_1_aa) = get_split(command.as_str(), ' ', 1) {
                    if let Some(u32_value_aa) = hex_str_to_u32(param_1_aa) {
                        rprintln!(
                            "Received background command for u32 addres {}",
                            u32_value_aa
                        );
                        //TODO change to proper command
                        Some(CLICommand::BackgroudTask(u32_value_aa as u8))
                    } else {
                        // 1st param was not hex value
                        None
                    }
                } else {
                    None
                }
            }
            _ => {
                // unknown command, return None
                None
            }
        }
    } else {
        // No command was given 0 index
        None
    }
}

/// Returns a string slice of the index place in the command split according to the given splitter.
/// It will not take into account leading and trailing splitter characters as wel as multiple following each other.
///
/// Presumes utf-8 encoding (ascii backwards compatible), which a String always is, as well as the heapless version in rust.
#[inline]
fn get_split(command: &str, splitter: char, index: u8) -> Option<&str> {
    // Counter for the current part = the current slice when split according to splitter
    let mut current_part_index = 0;

    // counter for the bytes, a char can be multiple bytes
    let mut current_byte_index = 0;
    let mut found_part = false;

    // Will be the INclusive start
    let mut current_slice_start = 0;
    // Will be the EXclusive end
    let mut current_slice_end = 0;

    // For removing trailing splitters
    // By setting this to true and index being 0, it is as if we start from -1
    // and have already encountered the first splitter.
    // We will eat the rest and start index 0 slice when we find first non splitter.
    let mut in_splitter_sequence: bool = true;

    for character in command.chars() {
        if character == splitter {
            // eat splitters following each other by doing nothing if in a sequence
            if !in_splitter_sequence {
                // When we encounter first splitter after sequence of non splitters

                in_splitter_sequence = true;

                // Will be the start of this char exlusive,
                // so everything up until and inclusive the last byte of the end char
                current_slice_end = current_byte_index;

                // before we increment the part index, check if the one
                // just completed is the one we wanted
                if current_part_index == index {
                    found_part = true;
                    break;
                }

                // increment slice index when a new splitter is encountered
                current_part_index += 1;
            }
        } else {
            // reset splitter sequence
            if in_splitter_sequence {
                // We will only enter this if if the previous char was the splitter.
                // So assign the current start here

                // start of new slice
                in_splitter_sequence = false;
                // Assign start
                current_slice_start = current_byte_index;
            }

            // Do nothing for chars inbetween
        }

        // update current byte index with the utf-8 size of the char
        current_byte_index += character.len_utf8();
    }

    // if we found it, return slice with the start and end indexes
    if found_part {
        Some(&command[current_slice_start..current_slice_end])
    } else {
        // Edge case: no trailing splitter.
        // Manually check if this could be the slice we want.
        if current_part_index == index {
            // Return tail of string when at wanted part but no trailing splitter
            Some(&command[current_slice_start..])
        } else {
            None
        }
    }
}

/// Turns str holding pure (no whitespace) hex into its 32bit unsigned value.
/// Leading 0x or 0 may be ommitted.
fn hex_str_to_u32(s: &str) -> Option<u32> {
    let mut value: u32 = 0;
    // Exponent is the index from right to left
    let mut exponent = 0;
    for c in s.chars().rev() {
        let factor: u8 = match c {
            '0' => 0,
            '1' => 1,
            '2' => 2,
            '3' => 3,
            '4' => 4,
            '5' => 5,
            '6' => 6,
            '7' => 7,
            '8' => 8,
            '9' => 9,
            'A' => 10,
            'B' => 11,
            'C' => 12,
            'D' => 13,
            'E' => 14,
            'F' => 15,
            'x' => break,
            _ => {
                // unexpected token, return None
                return None;
            }
        };

        // value += factor * 2^(exponent * 4)
        value = value + ((factor as u32) << (exponent * 4));
        exponent += 1;
    }
    Some(value)
}
