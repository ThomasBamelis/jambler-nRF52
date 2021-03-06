#![no_std]
#![no_main]

// TODO delete, warnings are a pain in the ass when trying to work quickly
//#![allow(warnings)]
#![allow(unused_variables)]
#![allow(dead_code)]

use crate::jambler::BlePhy;
use crate::jambler::ConnectionSample;
use crate::jambler::ConnectionSamplePacket;
use crate::jambler::JamblerReturn;
use nrf52840_hal as hal; // Embedded_hal implementation for my chip
                         //use panic_halt as _; // Halts on panic. You can put a breakpoint on `rust_begin_unwind` to catch panics.
                         // TODO change panic behaviour to turn on led on board, so we can spot it with multiple leds
use rtt_target::{rprintln, rtt_init_print}; // for logging to rtt

mod jambler;
use crate::jambler::nrf52840::{Nrf52840IntervalTimer, Nrf52840Jambler, Nrf52840Timer};
use crate::jambler::{Jambler, JamblerTask};

use crate::jambler::deduce_connection_parameters::{reverse_calculate_crc_init, DeduceConnectionParametersControl, DeductionState, CounterInterval};

mod serial;
use crate::serial::SerialController;
use heapless::spsc::Queue;
use heapless::{consts::*, String};

// Our pseudo PDU heap
use crate::jambler::{initialise_pdu_heap, PDU_SIZE};
const JAMBLER_RETURN_CAPACITY: u8 = 5;


// for the

// My own panick handler
// Rewrite this to start blinking a red LED on the board and to print the error message via RTT
use core::panic::PanicInfo;
use core::sync::atomic::{self, Ordering};

/// My own panic handler.
/// Wrote my own so I could print the error message on any medium I want.
/// This makes it so I do not have to work with results anywhere which slow down the process too much, this is a real time application.
///
/// I can also use it to make an LED blink, which will be very useful to show an error occurred when multiple devices are connected and I cannot hook them all up to JLink.
///
/// Inline(never) necessary to be able to set a breakpoint on rust_begin_unwind
#[inline(never)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Prints the panic over RTT
    // TODO remove all unnecesary RTT communication
    // TODO RTT is buffered, so if the buffer is not filled with anything I will be able to attack JLink and read out the error after the fact!
    rprintln!("{}", info);
    loop {
        atomic::compiler_fence(Ordering::SeqCst);
        // Makes a debugger stop here.
        cortex_m::asm::bkpt();
        // TODO blink something
    }
}

// This defines my rtic application, passing the nrf52840 hal to it.
// It also specifies we want access to the device specific peripherals (via ctx.devices = hal::peripherals)
#[rtic::app(device = crate::hal::pac, peripherals = true)]
const APP: () = {
    struct Resources {
        /// This struct holds all resources shared between tasks.

        /// Is used to control the deduce connection parameters task.
        /// Works a bit like controlling peripherals.
        /// This struct contains the "control registers" for the task.
        dcp_control: DeduceConnectionParametersControl,
        uarte: SerialController,
        jambler: Jambler<Nrf52840Jambler, Nrf52840Timer, Nrf52840IntervalTimer>,
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

        /* Showcasing on how to let nrf use PDU pools which give you boxes which let you pass around a BOX to tasks! Without having a heap! */
        // Reserve memory for the PDUs,
        static mut PDU_MEMORY_POOL: [u8; PDU_SIZE * 11] = [0; PDU_SIZE * 11];
        let PDU_POOL_MAX = unsafe { initialise_pdu_heap(&mut PDU_MEMORY_POOL) };

        /*
        // Will be 1 less than 10 due to alignment
        rprintln!("PDU pool real size = {}", PDU_POOL_MAX);

        // Allocate a PDU and initialise it (you can leave it uninitialised)
        // Will return None if memory full
        let mut boxed_pdu = PDU::alloc().unwrap().init([69; 258]);
        // Use it like a normal box (as if we had it right here)
        boxed_pdu[3] = boxed_pdu[1] + boxed_pdu[2];
        let changed = boxed_pdu[3];

        // give p as u32 to packetptr of nrf
        let p = boxed_pdu.as_ptr();
        let box_ref = &mut boxed_pdu;
        let p_as_u32 = box_ref.as_ptr() as u32;
        assert_eq!(p_as_u32, p as u32);

        // Showcase with raw read that it is the calculation of above
        let grabbed = unsafe {core::ptr::read_volatile(p.add(3))};

        // Have to free it manually (I think?)
        drop(boxed_pdu);
        // cannot use boxed_pdu after this point because drop moved it (destroyed it)
        */

        // setup uart
        // Configure uart according to adafruit feather board schematic
        // I had to hardcode the ports in the serial controller.
        let p0 = hal::gpio::p0::Parts::new(ctx.device.P0);
        let uart_pins = hal::uarte::Pins {
            txd: p0
                .p0_15
                .into_push_pull_output(hal::gpio::Level::High)
                .degrade(),
            rxd: p0.p0_13.into_floating_input().degrade(),
            cts: None,
            rts: None,
        };
        let uart_device: hal::pac::UARTE1 = ctx.device.UARTE1;
        let uarte = SerialController::new(uart_device, uart_pins);

        // setup jammer
        let radio: hal::pac::RADIO = ctx.device.RADIO;
        let nrf_jambler = Nrf52840Jambler::new(radio);
        let timer_per: hal::pac::TIMER2 = ctx.device.TIMER2;
        let nrf_timer = Nrf52840Timer::new(timer_per);
        let interval_timer_per: hal::pac::TIMER1 = ctx.device.TIMER1;
        let interval_nrf_timer = Nrf52840IntervalTimer::new(interval_timer_per);

        let jambler = Jambler::new(nrf_jambler, nrf_timer, interval_nrf_timer);

        // Spawn the late resources initialiser, so any initialisation for which the resources must be in their final memory place can be done there.
        ctx.spawn
            .initialise_late_resources(InitialisationSequence::InitialiseJambler)
            .ok();

        init::LateResources {
            dcp_control: DeduceConnectionParametersControl::new(),
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

    /*
    Priority explanation (0-7, 3 bit interrupt priority on arm cortex m4f)
    7 - jambler state interrupts (radio and interval timer (TODO unite, they only lock radio and do something, so they are sequential => share prio)
    6 - Inter-board communication: I2C and SPI handlers (never active at the same stage)
    5 - Remaining Interrupt: any remaining interrupts: long term timer wrapping, uart, etc..

     */

    /// A handler for radio interrupts.
    /// Is passed completely to jambler.
    #[task(binds = RADIO ,priority = 7, resources = [jambler], spawn = [handle_jambler_return])]
    fn handle_radio(ctx: handle_radio::Context) {
        // Interpret the resource (compiler comfort)
        let jambler: &mut Jambler<Nrf52840Jambler, Nrf52840Timer, Nrf52840IntervalTimer> =
            ctx.resources.jambler;

        // pass the interrupt and spawn the jambler return handler task with it immediately if there is a return value
        if let Some(jambler_return) = jambler.handle_radio_interrupt() {
            ctx.spawn.handle_jambler_return(jambler_return).expect(
                "JamBLEr handle return flooded. Panic because memory leak if this goes ok().",
            );
        }
    }

    /// Handles interrupts of the INTERVAL timer used by the jammer
    #[task(binds = TIMER1 ,priority = 6, resources = [jambler], spawn = [handle_jambler_return])]
    fn handle_timer1(mut ctx: handle_timer1::Context) {
        let mut return_instruction = None;

        // Get lock on the jammer to be able to call its timer interrupt.
        ctx.resources.jambler.lock(|jambler| {
            // Pass the interrupt to jambler
            jambler.handle_interval_timer_interrupt(&mut return_instruction);
        });

        // pass the interrupt and spawn the jambler return handler task with it immediately if there is a return value
        if let Some(jambler_return) = return_instruction {
            ctx.spawn.handle_jambler_return(jambler_return).expect(
                "JamBLEr handle return flooded. Panic because memory leak if this goes ok().",
            );
        }
    }

    /// A handler for UART1
    /// UART1 is only available on the nrf52840, sorry not sorry
    #[task(binds = UARTE1 ,priority = 5, resources = [uarte], spawn = [cli_command_dispatcher])]
    fn handle_uart(ctx: handle_uart::Context) {
        // get the resource
        let uarte: &mut SerialController = ctx.resources.uarte;

        if let Some(cli_command) = uarte.handle_interrupt() {
            ctx.spawn.cli_command_dispatcher(cli_command).unwrap();
        }
    }

    /// Handles interrupts of the timer used by the jammer.
    /// This is the timer used for long term timing, basically time keeping the system.
    #[task(binds = TIMER2 ,priority = 5, resources = [jambler])]
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

    /*
    Priority explanation:

    4 - Jambler interupt return handler: to handle a return value from jambler
    3 - I2C and SPI handlers: processing communication between
    2 - Everything that is "control related", the big guidelines of the execution
    1 - Heavy background processing
     */

    /// Takes heavy processing of jambler returns.
    /// For example:
    ///     - reverse calculating crc of freshly received packets
    ///     - Constructing output string for uart
    ///     -
    /// TODO: communicate via heapless::pool for packets
    /// TODO: make pool!(JamblerReturn : [JamBLErReturn ; capacity]);
    /// then grow in init
    ///
    /// WILL BE DIFFERENT FOR SLAVES AND MASTERS, DO THIS ONE IN HERE
    #[task(priority = 4, capacity = 5, resources = [jambler, dcp_control], spawn = [rtic_controller, deduce_connection_parameters])]
    fn handle_jambler_return(
        mut ctx: handle_jambler_return::Context,
        jambler_return: JamblerReturn,
    ) {
        //rprintln!("Handling jambler return value:{}", &jambler_return);
        match jambler_return {
            JamblerReturn::InitialisationComplete => {
                // Go to the next initialisation step, uart in this case for now
                ctx.spawn
                    .rtic_controller(RticControllerAction::NextInitialisationStep(
                        InitialisationSequence::InitialiseUart,
                    )).unwrap();
            }
            JamblerReturn::HarvestedSubEvent(harvested_subevent, completed_channel_chain) => {
                // turn the harvested subevent into a small and easily digested connection sample (reversing the crc is way too heavy to do in interrupt handler)

                /*
                rprintln!("Return handler START packet: {:?} | {}", harvested_subevent.packet.pdu.as_ptr(), harvested_subevent.packet.pdu[0]);
                if let Some(ref r) = harvested_subevent.response {
                    rprintln!("Response: {:?} | {}", r.pdu.as_ptr(), r.pdu[0]);
                }
                */

                let connection_sample: ConnectionSample;
                // Calculate the crc init values
                match &harvested_subevent.response {
                    None => {
                        // Process partial subevent
                        // check if 2 or 3 byte header, need to know for pdu length which we need for reversing the crc
                        let packet_pdu_length: u16;
                        if harvested_subevent.packet.pdu[0] & 0b0010_0000 != 0 {
                            packet_pdu_length = 3 + harvested_subevent.packet.pdu[1] as u16;
                        } else {
                            packet_pdu_length = 2 + harvested_subevent.packet.pdu[1] as u16;
                        }

                        connection_sample = ConnectionSample {
                            channel: harvested_subevent.channel,
                            time: harvested_subevent.time,
                            time_on_channel: harvested_subevent.time_on_the_channel,
                            packet: ConnectionSamplePacket {
                                first_header_byte: harvested_subevent.packet.pdu[0],
                                phy: harvested_subevent.packet.phy,
                                reversed_crc_init: reverse_calculate_crc_init(
                                    harvested_subevent.packet.crc,
                                    &harvested_subevent.packet.pdu[..],
                                    packet_pdu_length,
                                ),
                                rssi: harvested_subevent.packet.rssi,
                            },
                            response: None,
                        }
                    }
                    Some(response) => {
                        // process FULL harvested subevent

                        let master_pdu_length: u16;
                        if harvested_subevent.packet.pdu[0] & 0b0010_0000 != 0 {
                            master_pdu_length = 3 + harvested_subevent.packet.pdu[1] as u16;
                        } else {
                            master_pdu_length = 2 + harvested_subevent.packet.pdu[1] as u16;
                        }

                        let slave_pdu_length: u16;
                        if response.pdu[0] & 0b0010_0000 != 0 {
                            slave_pdu_length = 3 + response.pdu[1] as u16;
                        } else {
                            slave_pdu_length = 2 + response.pdu[1] as u16;
                        }

                        connection_sample = ConnectionSample {
                            channel: harvested_subevent.channel,
                            time: harvested_subevent.time,
                            time_on_channel: harvested_subevent.time_on_the_channel,
                            packet: ConnectionSamplePacket {
                                first_header_byte: harvested_subevent.packet.pdu[0],
                                phy: harvested_subevent.packet.phy,
                                reversed_crc_init: reverse_calculate_crc_init(
                                    harvested_subevent.packet.crc,
                                    &harvested_subevent.packet.pdu[..],
                                    master_pdu_length,
                                ),
                                rssi: harvested_subevent.packet.rssi,
                            },
                            response: Some(ConnectionSamplePacket {
                                first_header_byte: response.pdu[0],
                                phy: response.phy,
                                reversed_crc_init: reverse_calculate_crc_init(
                                    response.crc,
                                    &response.pdu[..],
                                    slave_pdu_length,
                                ),
                                rssi: response.rssi,
                            }),
                        }
                    }
                }

                /*
                rprintln!("Return handler END packet: {:?} | {}", harvested_subevent.packet.pdu.as_ptr(), harvested_subevent.packet.pdu[0]);
                if let Some(ref r) = harvested_subevent.response {
                    rprintln!("Response: {:?} | {}", r.pdu.as_ptr(), r.pdu[0]);
                }
                */

                // Make sure to release the PDUs from the pdu heap
                drop(harvested_subevent.packet);
                if let Some(response) = harvested_subevent.response {
                    drop(response)
                }

                // Push to the queue for the connection parameter deducer
                let queue: &mut Queue<ConnectionSample, U32> =
                    &mut ctx.resources.dcp_control.connection_sample_queue;

                if let Err(e) = queue.enqueue(connection_sample) {
                    rprintln!("WARNING: connection sample queue flooding, dropping sample.")
                }

                // If the deducer is not yet running, start it
                ctx.spawn.deduce_connection_parameters().ok();

                // TODO do something if you completed channel chain?
            }
            JamblerReturn::HarvestedUnusedChannel(channel, completed_channel_chain) => {
                // Push to the queue for the connection parameter deducer
                let queue: &mut Queue<u8, U32> =
                    &mut ctx.resources.dcp_control.unused_channel_queue;

                if let Err(e) = queue.enqueue(channel) {
                    rprintln!("WARNING: unused channel sample queue flooding, dropping sample.")
                }

                // If the deducer is not yet running, start it
                ctx.spawn.deduce_connection_parameters().ok();

                // TODO do something if you completed channel chain?
            }
            JamblerReturn::ResetDeducingConnectionParameters(
                new_access_address,
                master_phy,
                slave_phy,
            ) => {
                // Signal the task it has to reset
                ctx.resources.dcp_control.reset = true;
                ctx.resources.dcp_control.access_address = new_access_address;
                ctx.resources.dcp_control.master_phy = master_phy;
                ctx.resources.dcp_control.slave_phy = slave_phy;

                // If the deducer is not yet running, start it.
                // It has to be started to read out that it has to reset
                // Has only capacity 1, so ok() will let it run if not yet running or discard the capicity full error if it
                // is running already.
                ctx.spawn.deduce_connection_parameters().ok();
            }
            JamblerReturn::NoReturn => {}
        }
    }

    /// The central controller.
    ///
    /// Watch out, spawning a task with a higher priority will preempt the current one.
    /// Other tasks can pass requests to this task.
    ///
    /// The responsibility of this task is to be a central point to avoid code duplication.
    #[task(priority = 2, resources = [jambler, uarte], spawn = [ initialise_late_resources])]
    fn rtic_controller(
        ctx: rtic_controller::Context,
        rtic_controller_action: RticControllerAction,
    ) {
        match rtic_controller_action {
            RticControllerAction::NextInitialisationStep(next_step) => {
                ctx.spawn.initialise_late_resources(next_step).unwrap();
            } // TODO a user interrupt
              /*
              jambler.handle_user_interrupt();
              // reset the backgroudn worker
              *ctx.resources.reset_deduce_connection_parameters = true;
                  // If the deducer is not yet running, start it.
                  // It has to be started to read out that it has to reset
                  if !*ctx.resources.deduce_connection_parameters_is_running {
                      ctx.spawn.deduce_connection_parameters().ok();
                  }
              */
        }
        // TODO reset jambler, the deduce conn parameters task, send interrupt to slaves
    }

    /// Initialise the late resources after they have been copied into their static place.
    ///
    /// ANY INITIALISING THAT HAS TO BE DONE BY GIVING A POINTER TO A BUFFER IN A LATER RESOURCE HAS TO BE DONE HERE AND NOT IN INIT!
    ///
    /// Has same priority as command dispatcher for now.
    /// Interrupts are already enabled here for the processor.
    #[task(priority = 2, resources = [jambler, uarte])]
    fn initialise_late_resources(
        mut ctx: initialise_late_resources::Context,
        point_in_init: InitialisationSequence,
    ) {
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
    #[task(priority = 2, resources = [jambler, uarte], spawn = [ initialise_late_resources])]
    fn cli_command_dispatcher(mut ctx: cli_command_dispatcher::Context, command: String<U256>) {
        match parse_command(command) {
            Some(cli_command) => {
                match cli_command {
                    CliCommand::UserInterrupt => {
                        // TODO configure what happens on user interrupt
                        ctx.resources.jambler.lock(|jambler| {
                            jambler.execute_task(JamblerTask::UserInterrupt);
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
                    CliCommand::JamblerTask(jambler_task) => {
                        // propagate jambler command to jambler
                        ctx.resources.jambler.lock(|jambler| {
                            jambler.execute_task(jambler_task);
                        });
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

    /// A task with the lowest non-idle priority which will try to determine the connection parameters of a connection.
    ///
    /// It does this using as input a Queue of ConnectionSamples and a Queue of unused channel which will be filled by someone else.
    /// It indicates it is working by setting a boolean.
    /// All the above are RTIC resources to which a task has to get atomic access to write to them.
    ///
    /// This task is allowed to run very slowly.
    /// It just has to stay out of the way of other tasks.
    /// It is the most computationally expensive task by far because of the patter matching, but that is no problem
    #[task(priority = 1, resources = [dcp_control])]
    fn deduce_connection_parameters(mut ctx: deduce_connection_parameters::Context) {
        /*
            Declaring the local statics.
            "A static variable can be declared within a function, which makes it inaccessible outside of the function; however, this doesn't affect its lifetime (it isn't dropped at the end of the function)."
            So I think the initialisation is not done when this function starts, but at compile time and it is never run at runtime.
        */
        static mut DEDUCTION_STATE: DeductionState = DeductionState::new();


        /*                   BOOTING UP of the task                    */

        // Set locals used for control flow
        let mut new_information = true; // Assume we got called because of some new information


        /*                        Work loop                           */

        // As long as there was new information or we didn't try all intervals and the connection parameters have not been found, keep looping
        while new_information {
            // Check if we received a reset
            // TODO does this work? does this give a reference in the closure?
            let mut reset: bool = false;
            ctx.resources.dcp_control.lock(|dcp_control| {
                reset = dcp_control.reset;
            });
            if reset {
                // Reset the control block and get the access address it holds now
                let mut new_access_address: u32 = 0;
                let mut master_phy: BlePhy = BlePhy::Uncoded1M;
                let mut slave_phy: BlePhy = BlePhy::Uncoded1M;
                ctx.resources.dcp_control.lock(|dcp_control| {
                    let (na, mp, sp) = dcp_control.reset();
                    new_access_address = na;
                    master_phy = mp;
                    slave_phy = sp;
                });

                // reset deduction state (persistent between tasks)
                DEDUCTION_STATE.reset(new_access_address, master_phy, slave_phy);
            }

            // Automatically borrows &mut
            let mut opt_conn: Option<u32> = None;
            let mut opt_crci: Option<u32> = None;
            ctx.resources.dcp_control.lock(|dcp_control| {
                let optional_new_time_delta_or_crc_init = DEDUCTION_STATE.process_new_information_simple(
                &mut dcp_control.connection_sample_queue,
                &mut dcp_control.unused_channel_queue,
                );
                opt_conn = optional_new_time_delta_or_crc_init.0;
                opt_crci = optional_new_time_delta_or_crc_init.1;
            });

            new_information = false;

            if let Some(smallest_delta) = opt_conn {
                rprintln!("Sniffed packet (in same chunk) {} -> New smallest delta: {}", DEDUCTION_STATE.get_nb_packets() , smallest_delta);
            }

            if let Some(crc_init) = opt_crci {
                rprintln!("Sniffed packet (in same chunk) {} -> New crc init: {}", DEDUCTION_STATE.get_nb_packets(), crc_init);
            }


            // Do a run for a connection interval
            let (counter_result, other_params_option) = DEDUCTION_STATE.process_interval_simple();

            match &counter_result {
                CounterInterval::NoSolutions => {
                    rprintln!("No solutions, resetting self");
                    ctx.resources.dcp_control.lock(|dcp_control| {
                        dcp_control.reset = true;
                    });
                },
                CounterInterval::MultipleSolutions(_) => {
                    rprintln!("Not enough info after {} packets", DEDUCTION_STATE.get_nb_packets());
                },
                CounterInterval::ExactlyOneSolution(counter, _) => {
                    let (conn_interval, channel_map, absolute_time_found_counter, drift, crc_init) = other_params_option.expect("Other params not supplied on exactly one solution.");
                    let counter = *counter;
                    let aa = DEDUCTION_STATE.get_access_address();
                    let mp = DEDUCTION_STATE.get_master_phy();
                    let sp = DEDUCTION_STATE.get_slave_phy();
                    rprintln!("Exactly one solution! Report back:\nConn_interval: {}\nChannel map: {:#039b}\nAbsolute start time: {}us\nDrift since start {}us\nCounter at start: {}\nCrc init: {:#08X}\nAccess Address {}\nMaster phy: {}\nSlave phy: {}", conn_interval, channel_map, absolute_time_found_counter, drift, counter, crc_init, aa, mp, sp);
                },
                CounterInterval::Unknown => {
                }
            }


            // Check if we would have new information if we entered the next loop
            // If we have a new sample or if there is a reset
            ctx.resources.dcp_control.lock(|dcp_control| {
                if !(dcp_control.connection_sample_queue.is_empty()
                    && dcp_control.unused_channel_queue.is_empty())
                    || dcp_control.reset
                {
                    new_information = true;
                }
            });
        }
    }

    // The unused interrupt used for triggering software tasks.
    // Every separate task needs its own as far as I know.
    // There are 6 of them in total, so I can have 6 software tasks
    extern "C" {
        fn SWI0_EGU0();
        fn SWI1_EGU1();
        fn SWI2_EGU2();
        fn SWI3_EGU3();
        fn SWI4_EGU4();
        fn SWI5_EGU5();
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
enum RticControllerAction {
    NextInitialisationStep(InitialisationSequence),
}

/// Process jambler return values
#[inline]
fn process_jambler_return(jambler_return: Option<JamblerReturn>) -> Option<RticControllerAction> {
    if let Some(jr) = jambler_return {
        match jr {
            // If jambler reports his initialisation complete, move on to initialising uart
            JamblerReturn::InitialisationComplete => {
                return Some(RticControllerAction::NextInitialisationStep(
                    InitialisationSequence::InitialiseUart,
                ));
            }
            JamblerReturn::NoReturn => {},
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
enum CliCommand {
    JamblerTask(JamblerTask),
    UserInterrupt,
}

fn print_welcome_message(uarte: &mut SerialController) {
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

fn print_bootup_complete_message(uarte: &mut SerialController) {
    let mut welcome: String<U256> = String::new();
    welcome
        .push_str("\r\nInitialisation done.\r\nType a command and press enter:\r\n")
        .unwrap();
    uarte.send_string(welcome);
}

/// Helper function for parsing a uart string into a command.
/// Returns Some if the command had a valid syntax.
/// The command parameters might still be invalid though.
#[inline]
fn parse_command(command: String<U256>) -> Option<CliCommand> {
    if let Some(rtic_command) = get_split(command.as_str(), ' ', 0) {
        match rtic_command {
            "INTERRUPT" => Some(CliCommand::UserInterrupt),
            "discoveraas" => Some(CliCommand::JamblerTask(JamblerTask::DiscoverAas)),
            "jam" => {
                if let Some(param_1_aa) = get_split(command.as_str(), ' ', 1) {
                    if let Some(u32_value_aa) = hex_str_to_u32(param_1_aa) {
                        rprintln!("Received jam command for u32 addres {}", u32_value_aa);
                        //TODO change to proper command
                        Some(CliCommand::JamblerTask(JamblerTask::Jam))
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
    for (exponent, c) in s.chars().rev().enumerate() {
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
        value += (factor as u32) << (exponent * 4);
    }
    Some(value)
}
