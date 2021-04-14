use heapless::{consts::*, spsc::Queue, String};
use nrf52840_hal as hal;

use core::ptr::read_volatile;
use core::sync::atomic::{compiler_fence, Ordering::SeqCst};
use embedded_hal::digital::v2::OutputPin;
use rtt_target::rprintln;

/// A serial controller for the UARTE1 peripheral on the NRF52840
/// Furthermore, for the moment the tx and rx pins had to have been hardcoded.
/// They are hardcoded for the adafruit feather express (p0.24 rx, p0.25 tx).
///
/// The interrupt_handler function has to be called in your code wherever the interrupt handler for UARTE1 is called.
/// It will return a string containing a task (command) if the start_listening() function has been called and the interrupt character, the splitter character has been received or the 256 byte receive string is full.
/// To send a string call the send_string() function, passing it the heapless 256 byte string you want it to send (can be less, but the template is 256) and it will be added to the send queue of 1024 bytes.
/// At this point in my RTIC implementation I know none of these functions will interrupt one another, so I have paid no attention to the interrupt safety of the send_string function while it is sending (maybe queue problems).
///
/// It will send and receive bytes one by one.
///
/// The serial controller will do absolutely nothing itself to interrupt or reset anything when it reads an interrupt character.
/// When the handler returns a string holding INTERRUPT, you should start a task for interrupting or resetting or whatever you want to do to your chip.
pub struct SerialController {
    /// The peripheral giving me exclusive access to the uarte1.
    uarte1_peripheral: hal::pac::UARTE1,
    /// Unused, but still required because I do not know how to make those pins input/... as of now, the hal does this for me, I then manually set the rxd and txd pins.
    _pins: hal::uarte::Pins,
    /// Whether or not we are receiving a new command. If false, all input except the interrupt string will be ignored.
    receiving: bool,
    /// Indicates if we are sending right now.
    sending: bool,
    /// A string which new command characters are saved in.
    received_string: String<U256>,
    /// A queue for sending bytes. Bytes from your string from send_string are added here.
    send_buffer: Queue<u8, U1024, u16>,
    /// The character on which to stop and run the command when receiving a command.
    splitter: char,
    /// The interrupt character.
    interrupt: char,
    /// The 1 byte receive array for communicating where to put a received byte to the uarte peripheral.
    /// It is volatile as the uarte peripheral will write to this.
    rx_byte: [u8; 1],
    /// The 1 byte send array for communicating the byte to send to the uarte peripheral.
    /// The uarte peripheral will read from this.
    tx_byte: [u8; 1],
}

/// 1) Build a new controller
/// 2) Call start listening at end of init
/// 3) Call the interrupt handler in interrupt task
/// 4) If the handler returns a string, you should should execute the task corresponding to the task sent in the string.
/// 5) If you want to receive a task, you should call the init_receive_string function. Otherwise you will only receive the interrupt task. It will listen for one task and then again only for the interrupt.
/// 6) If you want to send a string, you should call the send_string method
impl SerialController {
    /// Creates a new serial controller sending and receiving on p0.24 rx, p0.25 tx.
    /// Changing UARTE1 to UARTE0 would make the controller usable by all nrf52 chips.
    /// Giving me the port and allowing them to specify the pins on the port would make this work for all boards.
    /// Putting the pins in the right configuration myself would have me to not use pins_ctrl.
    /// It will not be listening until the start_listening function has been called.
    /// It will not send until the send_string function has been called.
    pub fn new(device: hal::pac::UARTE1, mut pins_ctrl: hal::uarte::Pins) -> SerialController {
        // start RX and TX, we will always be communicating with the user for interrupts.

        // Conservative compiler fence to prevent optimizations that do not
        // take in to account actions by DMA. The fence has been placed here,
        // before any DMA action has started.
        compiler_fence(SeqCst);

        // Select pins, see 6.33.2. The pins have been set to their according input or output
        // cannot use psel_bits I have no idea why not...
        // the rx pin is p0.24, tx is p0.25, so 24 as u8, 25 as u32
        // if you decide to not use the hal, you have to set the pins correctly

        // Set the whole select register zero apart from the port and pin
        // then set the connect to connected
        // This has to be done before enabling the module.
        device.psel.rxd.write(|w| {
            unsafe { w.bits(13 as u32) };
            w.connect().connected()
        });

        // see 6.33.2, txd has to be high
        pins_ctrl.txd.set_high().unwrap();
        device.psel.txd.write(|w| {
            unsafe { w.bits(15 as u32) };
            w.connect().connected()
        });

        // disconnect cts and rts, hwfc is 0 by default
        device.psel.cts.write(|w| w.connect().disconnected());
        device.psel.rts.write(|w| w.connect().disconnected());

        // Enable UARTE instance.
        device.enable.write(|w| w.enable().enabled());

        // Configure parity to no parity.
        device.config.write(|w| {
            w.parity()
                .variant(hal::pac::uarte0::config::PARITY_A::EXCLUDED)
        });

        // Configure frequency to 9600 baud.
        device.baudrate.write(|w| {
            w.baudrate()
                .variant(hal::pac::uarte0::baudrate::BAUDRATE_A::BAUD9600)
        });

        // Enable interrupts for txend (txstopped before will make end fire as well),
        // and enable interrupts for rx as well because we always listen for the interrupt signal
        device
            .intenset
            .write(|w| w.endrx().variant(hal::pac::uarte0::intenset::ENDRX_AW::SET));

        // We have to enable all reading material here as well because we always listen for the interrupt task.

        // we work with a 1 byte buffer, which we will need to read out before the next byte comes (dont type to fast I guess, so the task can execute).
        //device.shorts.write(|w| w.endrx_startrx().enabled());

        // Conservative compiler fence to prevent optimizations that do not
        // take in to account actions by DMA. The fence has been placed here,
        // after all possible DMA actions have completed.
        compiler_fence(SeqCst);

        SerialController {
            uarte1_peripheral: device,
            _pins: pins_ctrl,
            receiving: false,
            sending: false,
            received_string: String::new(),
            send_buffer: Queue::u16(),
            splitter: '\r', // TODO figure out why minicom sends this instead of \n
            interrupt: '`',
            rx_byte: [0u8; 1],
            tx_byte: [0u8; 1],
        }
    }

    /// Has to be called exactly once to start listening.
    /// Maybe I should split this up in a public one which can only be called once.
    #[inline]
    pub fn start_listening(&mut self) -> () {
        compiler_fence(SeqCst);

        // Set up the DMA read pointer
        self.uarte1_peripheral
            .rxd
            .ptr
            .write(|w| unsafe { w.ptr().bits(self.rx_byte.as_ptr() as u32) });

        // we only read in one byte in our 1 byte long buffer
        self.uarte1_peripheral
            .rxd
            .maxcnt
            .write(|w| unsafe { w.maxcnt().bits(1 as _) });

        // Start UARTE Receive transaction.
        self.uarte1_peripheral.tasks_startrx.write(|w|
            // for some unknown reason the trigger function is not available to us.
            unsafe { w.bits(1) });
        compiler_fence(SeqCst);
    }

    /// Interrupt handler which will return a string if a command or interrupt has been received otherwise None.
    /// The command will be the received command or INTERRUPT for an interrupt.
    /// This also handles sending.
    #[inline]
    pub fn handle_interrupt(&mut self) -> Option<String<U256>> {
        compiler_fence(SeqCst);
        // get the last read byte and figure out if we receivend an interrupt because of an endtx or endrx
        let received_event: bool = self.uarte1_peripheral.events_endrx.read().bits() != 0;
        let sent_event: bool = self.uarte1_peripheral.events_endtx.read().bits() != 0;
        compiler_fence(SeqCst);
        //rprintln!("Uart interrupt: received {}, sent {}, last byte {}", received_event, sent_event, last_received_byte);

        if sent_event && self.sending {
            self.sending_string();
        }

        // if we received something sensible, interrupt if it was the interrupt char, or add it to the receiving string if listening. Backspace is not supported.
        if received_event {
            // reset event to help use next time
            compiler_fence(SeqCst);
            self.uarte1_peripheral.events_endrx.reset();

            // read volatile from the rx buffer
            let last_received_byte: u8 = unsafe { read_volatile(&mut self.rx_byte[0]) };
            // TODO Bug: first byte received is alwas \0

            compiler_fence(SeqCst);
            let mut retu = None;
            let new_char: char = core::char::from_u32(last_received_byte as u32).unwrap();

            if new_char == self.interrupt {
                // received interrupt char, start interrupt task
                let mut ret: String<U256> = String::new();
                ret.push_str("INTERRUPT").unwrap();
                retu = Some(ret);
            } else if self.receiving {
                // was not the interrupt char, add to string if receiving
                retu = self.receiving_string(new_char);
            }

            // Listen for the next char.
            self.start_listening();
            return retu;
        }

        // TODO uncomment below once in a while to be sure you are not waking up all te time.
        /*
        // if both events are not true, or there was a sent interrupt while the buffer is empty.
        let no_event = !received_event && !sent_event;
        let endtx_but_not_sending = sent_event && !self.sending;
        if no_event || endtx_but_not_sending {
            // now for debugging.
            if no_event {
                rprintln!("Uart interrupt generated without endtx or enrx");
            }
            if endtx_but_not_sending {
                rprintln!("Uart interrupt generated for endtx with empty buffer. End transmission if once, bug if multiple times.");

                // Reset the events.
                self.uarte1_peripheral.events_endtx.reset();
            }
        }
        */
        None
    }

    /// Initialises the radio so that it will start to receive a task
    /// and return a task once it has received the splitter character.
    pub fn init_receive_string(&mut self) -> () {
        // set it so receivements are handled in the interrupt handler
        self.receiving = true;
        // reset whatever you may have received up until now.
        // When typing a command and pressing interrupt it would continue with the other one
        self.received_string.clear();
    }

    /// What to do with the received char in the interrupt handler.
    #[inline]
    fn receiving_string(&mut self, new_char: char) -> Option<String<U256>> {
        // Assume communication is ascii
        // utf-8 would be possible because aurte can get 4 bytes at a time (1 max utf-8 character)
        // Then it is as simple as having the new byte be the next utf-8 char.

        if new_char == self.splitter {
            // Stop receiving and send the command
            Some(self.stop_receiving())
        } else if new_char == 8u8 as char {
            // If backspace character, pop from the receive string
            self.received_string.pop();
            None
        } else {
            // Append the received byte to the string.
            // If buffer full, flush it.
            match self.received_string.push(new_char) {
                Err(_) => {
                    rprintln!(
                        "Receive buffer full, dropped {} and flushed buffer.",
                        new_char
                    );
                    Some(self.stop_receiving())
                }
                _ => None,
            }
        }
    }

    /// Stops receiving a command and flushes the received string.
    #[inline]
    fn stop_receiving(&mut self) -> String<U256> {
        let return_string: String<U256> = self.received_string.clone();
        self.received_string.clear();
        self.receiving = false;
        return_string
    }

    /// Sends the given string.
    /// If the string does not fit in the tx buffer,
    /// a message is sent over rtt and the string is not sent.
    /// Could be unsafe to use when it can interleave with the interrupt handler, because it alters the send queue used by the handler.
    pub fn send_string(&mut self, s: String<U256>) -> () {
        // this can break with utf8
        // Add all bytes of the string to the output buffer.
        let bytes = s.into_bytes();
        // Do the check here, so you either write the whole string or you don't.
        if bytes.len() + self.send_buffer.len() as usize > self.send_buffer.capacity() as usize {
            // could not fit, do not send
            rprintln!(
                "Output buffer overflow. Ommitting string: {}",
                String::from_utf8(bytes).unwrap()
            );
        } else {
            for byte in bytes {
                match self.send_buffer.enqueue(byte) {
                    Ok(()) => {}
                    Err(_lost_byte) => {
                        rprintln!("Output buffer overflow, but it should never happen here!");
                        break;
                    }
                }
            }

            // If we are not sending yet, start sending

            if !self.sending {
                // manually trigger the sending behaviour normally for the handler
                // This will run at the priority of the caller, which will have a lock, as it should only be sent from a software task.
                // Either way it will not be interrupted
                // TODO find a more elegant way

                self.sending_string();
            }
        }
    }

    /// Called when a byte was sent or a new string has to be sent.
    /// It will send a new byte from the queue until the queue is empty.
    /// It will then stop the tx and set sending to false, so it does not accidentally get called in the interrupt handler.
    #[inline]
    fn sending_string(&mut self) -> () {
        match self.send_buffer.dequeue() {
            Some(byte) => {
                if !self.sending {
                    self.uarte1_peripheral
                        .intenset
                        .write(|w| w.endtx().variant(hal::pac::uarte0::intenset::ENDTX_AW::SET));

                    self.sending = true;
                }

                // But the byte in the send buffer
                self.tx_byte[0] = byte;

                // Tell compiler to do everything sequential
                compiler_fence(SeqCst);

                // Reset the events.
                self.uarte1_peripheral.events_endtx.reset();
                //self.uarte1_peripheral.events_txstopped.reset();
                self.uarte1_peripheral
                    .intenset
                    .write(|w| w.endtx().variant(hal::pac::uarte0::intenset::ENDTX_AW::SET));

                // we have to make sure the pointer is in ram
                // But as far as I know, the resources reside in ram, how could you otherwise execute code on them.
                self.uarte1_peripheral
                    .txd
                    .ptr
                    .write(|w| unsafe { w.ptr().bits(self.tx_byte.as_ptr() as u32) });

                // I will take the easy way first and send byte per byte.
                self.uarte1_peripheral
                    .txd
                    .maxcnt
                    .write(|w| unsafe { w.maxcnt().bits(1 as _) });

                // Start UARTE Transmit transaction.
                self.uarte1_peripheral.tasks_starttx.write(|w|
                        // `1` is a valid value to write to task registers.
                        unsafe { w.bits(1) });

                compiler_fence(SeqCst);

                //rprintln!("Sending uart byte {}", byte);
            }
            None => {
                // transmission ended, nothing more to send.
                // I dont think it is really necessary to disable it even more

                compiler_fence(SeqCst);
                // Lower power consumption by disabling the transmitter once we're
                // finished.
                self.uarte1_peripheral.intenclr.write(|w| {
                    w.endtx()
                        .variant(hal::pac::uarte0::intenclr::ENDTX_AW::CLEAR)
                });

                // Reset the events.
                self.uarte1_peripheral.events_endtx.reset();

                self.uarte1_peripheral.tasks_stoptx.write(|w|
                    // `1` is a valid value to write to task registers.
                    unsafe { w.bits(1) });
                // Wait for transmission to end.
                let mut txstopped;
                loop {
                    txstopped = self.uarte1_peripheral.events_txstopped.read().bits() != 0;
                    if txstopped {
                        break;
                    }
                }
                compiler_fence(SeqCst);

                self.sending = false;
            }
        }
    }
}
