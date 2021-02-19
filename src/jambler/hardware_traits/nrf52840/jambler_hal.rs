use crate::jambler::HalHarvestedPacket;
use nrf52840_hal as hal; // Embedded_hal implementation for my chip
use hal::pac::RADIO;

use crate::jambler::BlePHY;
use super::super::{JamBLErHal, JamBLErHalError};

use core::sync::atomic::{compiler_fence, Ordering::SeqCst};

use rtt_target::rprintln;

/// A struct for altering the radio module of the nrf52840.
/// This struct will be held in the JamBLEr struct which is supposed to be static and in ram.
/// So the having the buffers in here should be no problem, just like with the serial code.
pub struct Nrf52840JamBLEr {
    radio_peripheral: RADIO,
    // TODO adapt to maximum size to ever be received
    // TODO deliver payloads in heapless vectors (len) and make a packet struct to return 
    send_buffer : [u8; 300],
    receive_buffer : [u8; 300],
    /// For remembering for discovering AAs
    current_phy : Option<BlePHY>,
    current_channel : Option<u8>,
}

impl Nrf52840JamBLEr {
    pub fn new(radio : RADIO) -> Nrf52840JamBLEr {
        Nrf52840JamBLEr {
            radio_peripheral : radio,
            send_buffer : [0; 300], // has to be 258 at least = max pdu length
            receive_buffer : [0; 300], // has to be 258 at least = max pdu length
            current_phy : None,
            current_channel : None,
        }
    }

    /// Will return what is necessary for the frequency register to set the frequency to the given channel.
    /// If the channel is invalid it will default to 0.
    /// 
    /// Channel index is the one were 37, 38 and 39 are advertising addresses.
    /// 
    #[inline]
    fn channel_to_frequency_register_value(channel_index : u8 ) -> u8 {
        // See ble specification page 2864 to make sense of this
        if channel_index > 39 {
            return 4;
        }
        let add_to_2400;
        // channel zero start from 4, every channel has 2 mhz
        if channel_index < 11 {
            add_to_2400 = 4 + channel_index * 2;
        }
        // in reality channel 38 sits here, so jump over it
        else if channel_index < 37 {
            add_to_2400 = 6 + channel_index * 2;
        }
        // advertising channels, see specification
        else if channel_index == 37 {
            add_to_2400 = 2;
        }
        else if channel_index == 38 {
            add_to_2400 = 26;
        }
        else {
            // channel 39
            add_to_2400 = 80
        }
        add_to_2400
    }
}

// TODO IMPORTANT: YOU CAN READ THE CURRENT RADIO STATE FROM ITS STATE REGISTER

/// Implement the necessary tools for the jammer.
impl JamBLErHal for Nrf52840JamBLEr {
    #[inline]
    fn set_access_address(&mut self, aa: u32) -> Result<(), JamBLErHalError> {
        Ok(())
    }


    /// Configures the radio to listen for the preamble of packet as access address.
    /// This will make receiving address match on the preamble.
    /// The coded PHY requires even more hacky stuff.
    /// It will listen on the uncoded 1M because the modulation is the same,
    /// but the preamble is changed to the coded one. 
    /// In this way we can receive the raw bits and manually decode the necessary parts later in the read discovered address function, as you cannot turn this off on this chip.
    /// Should listen completely raw.
    /// 
    /// You know what is next, so set in rxIdle.
    /// 
    /// Radio will always be disabled because of prepare function when entering this.
    #[inline]
    fn config_discover_access_addresses(&mut self, phy : BlePHY, channel : u8) -> Result<(), JamBLErHalError> {


        if self.radio_peripheral.power.read().power().is_disabled() {
            rprintln!("ERROR: power disabled while harvest packet config change");
            panic!()
        }
        if !self.radio_peripheral.state.read().state().is_disabled() {
            rprintln!("ERROR: radio not disabled while harvest packet config change");
            panic!()
        }

        // Should only listen on general purpose channels. Remember u8 is unsigned.
        if channel > 36 {
            return Err(JamBLErHalError::InvalidChannel(channel));
        }



        let radio = &mut self.radio_peripheral;

        // First things that are the same for all

        // TODO tx power should not have to be set because we are only listening?
        // crc off by default after reset

        // Select reception on 0th of 0-7 possible AAs to listen for
        radio.rxaddresses.write(|w| w.addr0().enabled());

        // Set the frequency to the channel
        let freq = Nrf52840JamBLEr::channel_to_frequency_register_value(channel);
        radio.frequency.write(|w| unsafe {  w.frequency().bits(freq)}); 

        // Set the receive buffer
        let ptr = self.receive_buffer.as_ptr() as u32;
        radio.packetptr.write(|w| unsafe { w.packetptr().bits(ptr) });

        match phy {
            BlePHY::Uncoded1M => {
                radio.mode.write(|w| w.mode().ble_1mbit());

                // Set the address to match on to the preamble
                // Preamble can be either FF or 55 (preceded by 0s = silence) but we don't know that because it depends on the first bit of the access address.
                // That is why we have to shift when reading it
                // We will try to catch the control packet sent by the central in a connection event, we read the first part of the header for that as well. //TODO but what if encrypted connection?

                // write to correct base and prefixes for address 0
                radio.base0.write(|w| unsafe {w.bits(0)} );
                radio.prefix0.write(|w| unsafe {w.ap0().bits(0xAA)});

                // For a 4-byte addres = prefix + 3 base, blen should be 3.
                // Valid values are actually only 2-4
                // But we will set it to 1. This means we will match on air to any sequence of 00AA

                // By default, lflen, s0len and s1len are all 0
                // 8-bit preamble by default
                
                // we will not even try to get the length of the captured packet
                // so the chip won't know.
                // Set statlen to 10 to indicate to the chip it should always listen for 10 bits on air, should be enough here for access address and the start of the header. Because we don't know about the length set maxlen to 10 as well, so exactly 10 bits get captured.
                // Little endian and whitening/dewhitening are disabled by default
                radio.pcnf1.write(|w| unsafe{ w
                    .maxlen().bits(10)
                    .statlen().bits(10)
                    .balen().bits(1) // TODO Illegal to change to 1? try 2?
                });


            },
            BlePHY::Uncoded2M => {
                rprintln!("discovering 2m not implemented yet");
                radio.mode.write(|w| w.mode().ble_2mbit());
                // set 16-bit preamble in pcnf0!
            },
            BlePHY::CodedS2 => {
                rprintln!("discovering c2 not implemented yet");
                // TODO will be 1mbit actually, just here now to show you
                radio.mode.write(|w| w.mode().ble_lr500kbit());
                
            },
            BlePHY::CodedS8 => {
                rprintln!("discovering c8 not implemented yet");
                radio.mode.write(|w| w.mode().ble_lr125kbit());
                
            },
        }


        self.current_channel = Some(channel);
        self.current_phy = Some(phy);

        // Enable shorts. From disabled, to rxidle to rx and loop between rx and rxidle to listen immediately after receiving a packet.
        // Calling tasks_rxen will ramp-up, start listening, throw an end event when a packet is received and immediately start listening again.
        radio.shorts.write(|w| w.rxready_start().enabled().end_start().enabled()
        // Also enable the short for when listening and the address matches, quickly do an signal strength measurement for the rest of the packet so we have an idea how far he is. This can be read from the rssisample register when finished. This does not throw an event but is completed 0.25 micro seconds after the task is triggered so should be ok.
        .address_rssistart().enabled()
    );

        // Enable interrupts on receiving a packet = the end event
        radio.intenset.write(|w| w.end().set());

        Ok(())
    }

    /// Reads the access address from the receive buffer of you chip.
    /// Might be hacky for certain chips.
    #[inline]
    fn read_discovered_access_address(&mut self) -> Option<(u32, i8)> {
        // Disable read interrupt to stop interrupt from firing before new packet.
        self.radio_peripheral.events_end.reset();

        // nrf says it has to be reset always?
        let ptr = self.receive_buffer.as_ptr() as u32;
        self.radio_peripheral.packetptr.write(|w| unsafe { w.packetptr().bits(ptr) });

        // read rssi. Value between 0 and 127. Should be made negative (rssi is always represented negative). RSSI = - rssisample dBm
        let rssi : i8 = - ( (self.radio_peripheral.rssisample.read().bits() as u8) as i8);

        let mut aa : u32;

        let mut received_bytes : [u8; 10] = [0;10];
        received_bytes.copy_from_slice(&self.receive_buffer[..10]);

        // TODO decipher what is now in the rx_buffer. See main lin 508 and further damien
        match self.current_phy.unwrap() {
            BlePHY::Uncoded1M => {
                // received buffer now contains the aa in its first 4 bytes
                // and the 16 first bits of the pdu = header
                // Use header to see if the packet is the control pdu we are looking for

                // shift because we do not know the preamble, could have been 55
                for _ in 0..3 {
                    let (first_header_byte, second_header_byte) =
                        dewithen_16_bit_pdu_header(received_bytes[4], received_bytes[5], self.current_channel.unwrap());
                    
                    aa = (received_bytes[3] as u32) << 24 | (received_bytes[2] as u32) << 16 | (received_bytes[1] as u32) << 8 | received_bytes[0] as u32;

                    
                    
                    if is_valid_discover_header(first_header_byte, second_header_byte) && is_valid_aa(aa, BlePHY::Uncoded1M) {
                        //TODO delete
                    rprintln!("Considered packet: AA {:#010x} with rssi {} header {:#010b} {:#010b}", aa, rssi, first_header_byte, second_header_byte);
                        return Some((aa, rssi))
                    }

                    // Not found, shift right, we might have been misaligned by having AA preamble but 55 could match this too
                    // Shift right but let the left hole be filled by the bit that will get kicked out for the next byte in the buffer
                    // do not do this for last byte
                    for i in 0..9 {
                        // shift it right
                        received_bytes[i] = received_bytes[i] >> 1;
                        // Fill left bit with the one that will get kicked ou in next iteration
                        // See this correcting for having received too soon
                        received_bytes[i] |= (received_bytes[i+1] & 0b0000_0001) << 7;
                    }
                }


            },
            BlePHY::Uncoded2M => {
                rprintln!("discovering 2m not implemented yet");

            },
            BlePHY::CodedS2 => {
                rprintln!("discovering c2 not implemented yet");

            },
            BlePHY::CodedS8 => {
                rprintln!("discovering c8 not implemented yet");

            },
        }

        None
    }

    /// Start sending with the current configuration.
    /// Radio should be configure before this.
    /// Should be called shortly after config and fire up very fast, so any speedup achieved by making the radio more ready but consume more power should already running.
    #[inline]
    fn send(&mut self) {
    }

    /// Start receiving with the current configuration.
    /// Radio should be configured before this.
    /// Should be called shortly after config and fire up very fast, so any speedup achieved by making the radio more ready but consume more power should already running.
    #[inline]
    fn receive(&mut self) {
        // This assumes to rxready_start short is set.
        // Could set a check here for that
        self.radio_peripheral.tasks_rxen.write(|w| w.tasks_rxen().set_bit());
    }

    /// Puts the radio in disabled state
    #[inline]
    fn reset(&mut self) {

        /*
        // Disable all interrupts
        let radio = &mut self.radio_peripheral;

        // Disable all interrupt. 
        // Indeed this is overkill,
        // but imptimisations can come later.
        // Now I just want certainty.
        // If this causes timing issues I will be able to detect them using my global timer which is accurate 1 microsecond with only 60 ppm.
        radio.intenclr.write(|w| w
            .ready().clear()
            .address().clear()
            .payload().clear()
            .end().clear()
            .disabled().clear()
            .devmatch().clear()
            .devmiss().clear()
            .rssiend().clear()
            .bcmatch().clear()
            .crcok().clear()
            .crcerror().clear()
            .framestart().clear()
            .edend().clear()
            .edstopped().clear()
            .ccaidle().clear()
            .ccabusy().clear()
            .ccastopped().clear()
            .rateboost().clear()
            .txready().clear()
            .rxready().clear()
            .mhrmatch().clear()
            .phyend().clear()
        );

        compiler_fence(SeqCst);
        // Get the radio into an idle state before resetting its registers
        // Reset the disabled event flag.
        radio.events_disabled.reset();
        compiler_fence(SeqCst);
        // You can enter the disabled state from any state
        radio.tasks_disable.write(|w| w.tasks_disable().set_bit());
        compiler_fence(SeqCst);
        // Wait for the radio to be actually disabled
        while radio.events_disabled.read().bits() != 0 {}
        compiler_fence(SeqCst);

        // Radio disabled and interrupts off -> reset all registers.

        // Configuration registers
        // Any fields not present here are read only fields.
        // Rust protected me from writing to them :)
        radio.shorts.reset();
        radio.intenset.reset();
        radio.intenclr.reset();
        radio.packetptr.reset();
        radio.frequency.reset();
        radio.txpower.reset();
        radio.mode.reset();
        radio.pcnf0.reset();
        radio.pcnf1.reset();
        radio.base0.reset();
        radio.base1.reset();
        radio.prefix0.reset();
        radio.prefix1.reset();
        radio.txaddress.reset();
        radio.rxaddresses.reset();
        radio.crccnf.reset();
        radio.crcpoly.reset();
        radio.crcinit.reset();
        radio.tifs.reset();
        radio.datawhiteiv.reset();
        radio.bcc.reset();
        for dab in radio.dab.iter() {dab.reset();};
        for dap in radio.dap.iter() {dap.reset();};
        radio.dacnf.reset();
        radio.mhrmatchconf.reset();
        radio.mhrmatchmas.reset();
        radio.modecnf0.reset();
        radio.sfd.reset();
        radio.edcnt.reset();
        radio.edsample.reset();
        radio.ccactrl.reset();




        // Event registers

        */

        // alternative: see page 353 of datasheet -> power on and of will reset the peripheral to its initial state. Will be in idle mode.
        
        // power off
        self.radio_peripheral.power.write(|w| w.power().disabled());
        // and back on again, should reset the whole peripheral including interrupts.
        self.radio_peripheral.power.write(|w| w.power().enabled());
    }

    /// Should prepare the radio for a configuration change.
    /// This might be a reset, but that may be too harsh.
    /// Any configurations between the previous reset and now should remain the exact same.
    /// It is more to safely change the access address for example and maybe the chip requires you should not be sending.
    #[inline]
    fn prepare_for_config_change(&mut self) {

        // If the radio is not disabled, disable it
        if !self.radio_peripheral.state.read().state().is_disabled() {
            // Get the radio into an idle state before resetting its registers
            // Reset the disabled event flag.
            self.radio_peripheral.events_disabled.reset();
            compiler_fence(SeqCst);
            // You can enter the disabled state from any state
            self.radio_peripheral.tasks_disable.write(|w| w.tasks_disable().set_bit());
            compiler_fence(SeqCst);
            // Wait for the radio to be actually disabled
            while self.radio_peripheral.events_disabled.read().bits() == 0 {}
            self.radio_peripheral.events_disabled.reset();
        }
    }

    /// Should "pause" the radio, stopping any interrupt from being received.
    /// Should not change anything to the configuration and does not need to be a low power mode.
    #[inline]
    fn idle(&mut self) {
        // Disable all interrupts
        self.radio_peripheral.intenclr.write(|w| w
            .ready().clear()
            .address().clear()
            .payload().clear()
            .end().clear()
            .disabled().clear()
            .devmatch().clear()
            .devmiss().clear()
            .rssiend().clear()
            .bcmatch().clear()
            .crcok().clear()
            .crcerror().clear()
            .framestart().clear()
            .edend().clear()
            .edstopped().clear()
            .ccaidle().clear()
            .ccabusy().clear()
            .ccastopped().clear()
            .rateboost().clear()
            .txready().clear()
            .rxready().clear()
            .mhrmatch().clear()
            .phyend().clear()
        );

        // And go to disabled state. Watch out for necessary ramp up

        // Get the radio into an idle state before resetting its registers
        // Reset the disabled event flag.
        self.radio_peripheral.events_disabled.reset();
        compiler_fence(SeqCst);
        // You can enter the disabled state from any state
        self.radio_peripheral.tasks_disable.write(|w| w.tasks_disable().set_bit());
        compiler_fence(SeqCst);
        // Wait for the radio to be actually disabled
        while self.radio_peripheral.events_disabled.read().bits() != 0 {}
        self.radio_peripheral.events_disabled.reset();
    }



    /*   // ***           packet harvesting               *** */


    /// Should configure the radio to receive all packets sent by the given 
    /// access address on the given phy and channel.
    /// Should enable crc checking (but not ignore failed checks) if the given crc_init is Some. Otherwise none.
    fn config_harvest_packets(&mut self, access_address: u32, phy: BlePHY, channel: u8, crc_init : Option<u32>) -> Result<(), JamBLErHalError> {

        if self.radio_peripheral.power.read().power().is_disabled() {
            rprintln!("ERROR: power disabled while harvest packet config change");
            panic!()
        }
        if !self.radio_peripheral.state.read().state().is_disabled() {
            rprintln!("ERROR: radio not disabled while harvest packet config change");
            panic!()
        }

        match crc_init {
            None => {
                rprintln!("Configure harvest packets\naa 0x{:08X}\nphy {:?}\nchannel {}\ncrc None", access_address, phy, channel);

            }
            Some(c) => {
                rprintln!("Configure harvest packets\naa 0x{:08X}\nphy {:?}\nchannel {}\ncrc 0x{:06X}", access_address, phy, channel, c);

            }
        }

        // Should only listen on general purpose channels. Remember u8 is unsigned.
        if channel > 36 {
            return Err(JamBLErHalError::InvalidChannel(channel));
        }

        let radio = &mut self.radio_peripheral;


        compiler_fence(SeqCst);

        // Set access address
        // Select reception on 0th of 0-7 possible AAs to listen for
        radio.rxaddresses.write(|w| w.addr0().enabled());
        // Fill this 0th AA with the given access address
        radio.base0.write(|w| unsafe{w.bits(access_address << 8)});
        radio.prefix0.write(|w| unsafe {w.ap0().bits((access_address >> 24) as u8)} );

        // Set the PHY mode and the corresponding preamble and cilen and termlen
        match phy {
            BlePHY::Uncoded1M => {
                radio.mode.write(|w| w.mode().ble_1mbit());
                radio.pcnf0.write(|w| unsafe{w.plen()._8bit().cilen().bits(0).termlen().bits(0)});
            },
            BlePHY::Uncoded2M => {
                radio.mode.write(|w| w.mode().ble_2mbit());
                radio.pcnf0.write(|w| unsafe{w.plen()._16bit().cilen().bits(0).termlen().bits(0)});
            },
            BlePHY::CodedS2 => {
                radio.mode.write(|w| w.mode().ble_lr500kbit());
                radio.pcnf0.write(|w| unsafe{w.plen().long_range().cilen().bits(2).termlen().bits(3)});
            },
            BlePHY::CodedS8 => {
                radio.mode.write(|w| w.mode().ble_lr125kbit());
                radio.pcnf0.write(|w| unsafe{w.plen().long_range().cilen().bits(2).termlen().bits(3)});
            }
        }

        // Set the frequency to the channel
        let freq = Nrf52840JamBLEr::channel_to_frequency_register_value(channel);
        radio.frequency.write(|w| unsafe {  w.frequency().bits(freq)}); 

        // Set dewhitening initial value to channel
        // The mask is unnecessary but beautiful
        radio.datawhiteiv.write(|w| unsafe{w.datawhiteiv().bits(0b01000000 | channel)});

        // Set the receive buffer
        let ptr = self.receive_buffer.as_ptr() as u32;
        radio.packetptr.write(|w| unsafe { w.packetptr().bits(ptr) });




        // Set the shortcuts of the state machine
        radio.shorts.write(|w| w
            // when rx enable task is triggered
            // start listening immediately after ramp up
            .rxready_start().enabled()
            // after a packet is received, immediately 
            .end_start().enabled()
            // when the access address matches, take an rssi sample of the signal
            .address_rssistart().enabled()
            // no information on this is in the datasheet
            // but for safety I will inlcude it.
            // This is the only rssistop shortcut I see
            // but it quite speaks for itself.
            .disabled_rssistop().enabled()
        );

        // Enable interrupts on receiving a packet = the end event
        radio.intenset.write(|w| w.end().set());

        // Everything under here was different in damiens code
        // however, for some parts I really do not see why he did what he did, it seems very random
        // I will configure it as I think is normal and see if it works

        // Set the header of the PDU for BLE so that the radio peripheral can read the length of the packet sent
        // s1incl is 0 automatic and crcinc is 0 (crc not in len as in ble) is automatic as well
        // WE DO INCLUDE S1 ALWAYS BECAUSE THE DATA PHYSICAL PACKET HEADER MIGHT BE 3 BYTES AND WE CANNOT SET THE PAYLOAD TO MORE THAN 255
        radio.pcnf0.write(|w| unsafe{w.s0len().bit(true).lflen().bits(8).s1len().bits(8)});

        // enable packet whitening (little endian is default) and set address length
        radio.pcnf1.write(|w| unsafe { w
            // 4-byte access address = 3-Byte Base Address + 1-Byte Address Prefix
            .balen().bits(3)
            // Enable Data Whitening over PDU+CRC
            .whiteen().set_bit()
        });

        // From the rubble source code
        // `x^24 + x^10 + x^9 + x^6 + x^4 + x^3 + x + 1`
        //pub const CRC_POLY: u32 = 0b00000001_00000000_00000110_01011011;

        // you can always configure the crc poly, no harm in that
        // the x^24 bit will be ignored, but by the way crc is implemented on the radio peripheral it will still be okay.
        const CRC_POLY: u32 = 0b00000001_00000000_00000110_01011011;
        radio.crcpoly.write(|w| unsafe {w.crcpoly().bits(CRC_POLY & 0x00FFFFFF)});

        // to be shure no misunderstandings happen, reset the registers which can be altered differently by a different control flow
        radio.crccnf.reset();
        radio.crcinit.reset();

        match crc_init {
            Some(crc_init_value) => {

                // we have a crc, set it
                // config the crc
                radio.crccnf.write(|w| {
                    // skip address since only the S0, Length, S1 and Payload need CRC. This corresponds to the PDU of the packet which corresponds to the BLE specification.
                    // 3 Bytes = CRC24
                    w.skipaddr().skip().len().three()
                });

                // set crc init
                radio.crcinit.write(|w| unsafe{w.crcinit().bits(crc_init_value & 0x00FFFFFF)});

                //TODO other stuff that is different, packet length maybe
                // Receive the whole packet as you normally would
                // Not strictly necessary but more clear
                
                // The header can be 16 or 24 bits
                // depending on the 0b0000_0100 bit, indicating CTE info field presence (24-bit header, new last byte).
                // S0 and LEN field always take care of the first 16 bits
                // However the payload might contain this extra byte first.
                // This means the maximum nrf payload has to be the maximum BLE payload + 1 byte = 255 + 1 = 256  

                // However!!!!!! The maximum payload length is 255. 
                // For this reason S1 has always been included to account for the 255 scenario. This might means that if the Length is 0 the first byte of the crc might be picked up as well, but that is no problem
                
                radio.pcnf1.write(|w| unsafe { w
                    // Set max payload to 255 (highest possible)
                    .maxlen().bits(255 as u8)
                    // disable static length (packet LENGTH + statlen receiving)
                    .statlen().bits(0 as u8)
                });
            }
            None => {
                // set CRC to disabled to be sure
                // Setting the length to 0 disables crc checking
                radio.crccnf.write(|w| w.len().disabled());

                // Whatever we receive, we want 3 bytes more to get the crc value
                // This means that we will have to check the length field of the packet to find were in the buffer the crc will be (and if it fits in 255-3).
                radio.pcnf1.write(|w| unsafe { w
                    // Set max payload to 255 (highest possible)
                    .maxlen().bits(255 as u8)
                    // Always receive 3 bytes more (the crc) than the LENGTH field of the incoming packet specifies.
                    .statlen().bits(3 as u8)
                });
            },
        }


        compiler_fence(SeqCst);

        // TODO delete. This is stuff I tried to start receiving packets as I should. Read out the main registers
        radio.txpower.write(|w| w.txpower()._0d_bm());

        rprintln!("Configurations after config:\nMode {:034b}\nModeconf {:034b}\nPCNF0 {:034b}\nPCNF1 {:034b}\nBase0 {:034b}\nPrefix0 {:034b}\nRXaddresses {:034b}\nShorts {:034b}\nInterrupts {:034b}", radio.mode.read().mode().bits(), radio.modecnf0.read().bits(), radio.pcnf0.read().bits(), radio.pcnf1.read().bits(), radio.base0.read().bits(), radio.prefix0.read().bits(), radio.rxaddresses.read().bits(), radio.shorts.read().bits(), radio.intenset.read().bits());

        let f = radio.frequency.read().frequency().bits();
        rprintln!("Channel frequency {} = {}MHz. (map is default = {})", f, 2000 + f as u32, radio.frequency.read().map().is_default());


        Ok(())
    }

    /// Returns Some if the packet just received was the first packet in the connection event and the boolean inside is true if the crc check was passed, false otherwise.
    /// Otherwise None.
    fn handle_harvest_packets_radio_interrupt(&mut self, calculate_crc_init : bool) -> Option<HalHarvestedPacket> {

        //TODO delete
        rprintln!("Logical address match: {} (should be 1)", self.radio_peripheral.rxmatch.read().bits());


        compiler_fence(SeqCst);
        // Reset the END event
        self.radio_peripheral.events_end.reset();

        // nrf says the ptr has to be reset always, so do it
        let ptr = self.receive_buffer.as_ptr() as u32;
        self.radio_peripheral.packetptr.write(|w| unsafe { w.packetptr().bits(ptr) });

        // read rssi. Value between 0 and 127. Should be made negative (rssi is always represented negative). RSSI = - rssisample dBm
        let rssi : i8 = - ( (self.radio_peripheral.rssisample.read().bits() as u8) as i8);


        compiler_fence(SeqCst);

        let crc_error_option: Option<bool>;
        let crc_init_option: Option<u32>;

        // find out if the we were instructed to check the crc by checking if we enabled it before or not
        let crc_check_disabled = self.radio_peripheral.crccnf.read().len().is_disabled();
        if !crc_check_disabled {
            let crc_error = self.radio_peripheral.crcstatus.read().crcstatus().is_crcerror();
            crc_error_option = Some(crc_error);
        }
        else {
            crc_error_option = None;
        }

        // If we were instructed and if we can, reverse calculate the crc

        // payload overflow (if payload was bigger than 252 and we wanted 3 extra with statlen)
        let maxlen_overflow = self.radio_peripheral.pdustat.read().pdustat().is_greater_than();
        // Alternative: pdu length + 3 = written to read buffer length

        if calculate_crc_init && !maxlen_overflow {
            // TODO I could just enable crc and read the (check failed) crc from the rxcrc register right?
            let mut pdu_length : u16;
        
            // Check 16 or 24 bit header
            // Check S0 holding the first header byte with CP indicating 24-bit headre or not.
            // TODO try if it is the same layout as shown in the specs if you set it to big endian, now have to reverse it if I look at damiens code.
            // TODO see vol 6 part B sec 1.2 Bit Ordering
            //let cte_info_header_byte_present : bool = self.receive_buffer[0] & 0b0000_0100 != 0;
            let cte_info_header_byte_present : bool = self.receive_buffer[0] & 0b0010_0000 != 0;

            if cte_info_header_byte_present {
                // 24-bit = 3 byte header
                pdu_length = 3;                
            }
            else {
                // 16-bit = 2 byte header
                pdu_length = 2;
            }

            // Get payload length
            let payload_length = self.receive_buffer[1] as u16;
            pdu_length += payload_length;

            // Get received CRC calue
            // Remember: the CRC is most significant byte and bit over the air while the payload is received least significant bit and byte.
            // This means that the first byte of the actual payload will be sent last on air
            // This causes the sniffer to interpret this wrong!
            // We need to reverse the bits and the first byte will be the last byte of the CRC
            // TODO wrong under here HOWEVER damiens code does not do this and maybe the crc reverse needs it this (wrong) way!
            // NRF datasheet: the static payload add-on sits between the payload and CRC = appended to it. Looks like it is put in the buffer as received on air
            
            // Sits in the buffer right after the PDU, and indexes start from 0:
            let crc_buffer_offset = pdu_length as usize;
            let crc_value = 
                (self.receive_buffer[crc_buffer_offset] as u32) |
                (self.receive_buffer[crc_buffer_offset + 1] as u32) << 8 |
                (self.receive_buffer[crc_buffer_offset + 2] as u32) << 16;
            
            /*
            let crc_value = 
                (reverse_bits(self.receive_buffer[2]) as u32) |
                (reverse_bits(self.receive_buffer[1]) as u32) << 8 |
                (reverse_bits(self.receive_buffer[0]) as u32) << 16;
            */

            // calculate crc init
            // give them a buffer where you step over th 3 CRC bytes at the beginning
            let crc_init = reverse_calculate_crc_init(crc_value, &self.receive_buffer[3..], pdu_length);
            crc_init_option = Some(crc_init);
        }
        else {
            crc_init_option = None;
        }

        const print : bool = true;
        if print {
            let mut pdu_length : u16;
            let cte_info_header_byte_present : bool = self.receive_buffer[0] & 0b0010_0000 != 0;
            if cte_info_header_byte_present {
                pdu_length = 3;                
            }
            else {
                pdu_length = 2;
            }
            let payload_length = self.receive_buffer[1] as u16;
            pdu_length += payload_length;


            rprintln!("Received packet, crc enabled = {}", !crc_check_disabled);
            //TODO change back to pdu_length
            for i in 0..2 {
                rprintln!("{:#02x} {:#010b} byte {}",self.receive_buffer[i as usize],self.receive_buffer[i as usize],i);
            }

            // get the crc as the chip thinks it is
            if !crc_check_disabled {
                rprintln!("Received crc: {:#08x} = {:#026b}", self.radio_peripheral.rxcrc.read().rxcrc().bits(), self.radio_peripheral.rxcrc.read().rxcrc().bits());
            }
            if let Some(crc_error) = &crc_error_option {
                rprintln!("Crc check succeeded: {}", !crc_error);
            }
            if let Some(ini) = &crc_init_option {
                rprintln!("Calculated reversed crc init: {:#08x} = {:#026b}", ini, ini);
            }
        }

        // return it
        // Always return some, we have configured the radio to only receive interrupts on packet reception.
        Some(HalHarvestedPacket {
            crc_ok : crc_error_option,
            crc_init : crc_init_option,
            rssi,
            first_header_byte : self.receive_buffer[0],
            second_header_byte : self.receive_buffer[1],
        })

        //TODO if you can access crc (no maxlen oveflow), you can reverse it with the entire pdu including header. Becuase the next packet ist still 150 micros away, we might be able to do a for loop with max length of 258 before that hits in this radio interrupt handler. Remember the radio is already listening again because of the short. This circumvents returning the whole packet all the way back to jambler. But it is very dirty and maybe not extensible to other chips


        // TODO dont forget to add these function in the harvest_packets functions
        // TODO adapt the return of this interface function to your needs

    }


    //TODO WHEN SENDING ON BLE CODED PHY YOU HAVE TO USE PHYEND SHORTCUT AND EVENT!


}











/***************************************************/
/* // ***          UTILITY  FUNCTIONS          *** */
/***************************************************/


//TODO move some to jambler bit stream processing file
/// BTLE CRC reverse routine, originally written by Mike Ryan,
/// Dominic Spill and Michael Ossmann, taken from ubertooth_le.
/// 
/// TODO test this P3089 BLE
fn reverse_calculate_crc_init(received_crc_value : u32, pdu: & [u8], pdu_length : u16) -> u32 {

    let mut state : u32 = received_crc_value;
	let lfsr_mask: u32 = 0xb4c000;

	for i in (0..pdu_length).rev() {
		let cur : u8 = pdu[i as usize];
		for j in 0..8 {
            // crc = 24 bit, 24th will be rightmost bit
			let top_bit : u8 = (state >> 23) as u8; 
			state = (state << 1) & 0xffffff;
			state |= (top_bit ^ ((cur >> (7 - j)) & 1)) as u32;
			if top_bit != 0 {
				state ^= lfsr_mask;
            }
		}
	}

	let mut ret : u32 = 0;
	for i in 0..24 {
		ret |= ((state >> i) & 1) << (23 - i);
    }

	return ret;
}



//TODO wrong for sure
// From Damien Cauquil
/// See figure 3.5 of specification page 2925.
/// The whitening and dewithening is the same, so just implement the figure.
fn dewithen_16_bit_pdu_header(first_byte : u8, second_byte : u8, channel : u8) -> (u8, u8) {
    // Could change this to wanted pdu length later if you would need it again.
    let mut pdu = [first_byte, second_byte];
    // Initialise according to the spec sheet.
    // 6 rightmost (lsb) bits are set to the channel and 7th (right to left = second most significant) is one.
    // If the channel is valid it will fit in its 6 rightmost bits.
    // The leftmost bit (MSB) is never used
    let mut linear_feedback_shift_register : u8 = channel | 0b0100_0000;

    for byte in pdu.iter_mut() {
        for bit_index in 0..8 {
            // Get data out from xor 6th = rightmost bit and data in
            let x7 : bool = (linear_feedback_shift_register & 0b0000_00001) == 0b0000_0001;

            if x7 {
                // bit index has to be xored with 1
                // Do bitwise xor (0 in xor is stay the same for other side)
                *byte ^= 0b1 << bit_index;
            }

            // shift register next shift and operation
            linear_feedback_shift_register = linear_feedback_shift_register >> 1;
            // If the bit that will be shifted out was one, the XOR and shift will matter
            if x7 {
                // x1 to postion 0 will be 1
                linear_feedback_shift_register |= 0b0100_0000;
                // Position 4 will be XORed with one (3 is already in it)
                // If position 4 is 1, it will have to be set to 0 because it will be 1 xored with 1. If 0 it will be one because 0 xored with 1
                if (linear_feedback_shift_register & 0b0000_0100) == 0b0000_0100 {
                    // 1 XOR 1, set it to 0
                    linear_feedback_shift_register =  linear_feedback_shift_register & 0b1111_1011;
                }
                else {
                    //now 0 in it but xor with 1, set to 1
                    linear_feedback_shift_register =  linear_feedback_shift_register | 0b0000_0100;

                }
            }
        }
    }

    (first_byte, second_byte)
}

//TODO
/// Should be easy to put it al in one loop an reuse current bit mask en previous was 1
/// For now like this to not introduce bugs early for no reason.
#[inline]
fn is_valid_aa(aa : u32, phy : BlePHY) -> bool {
    // TODO change, debugging to listen to mine
    
    if aa == 0x8E89BED6 {
        return true;
    }
    else {
        return false;
    }

    // not more then 6 consecutive 0s
    let mut zero_count = 0;
    for bit_index in 0..32 {
        if (aa & (0b1 << bit_index)) == 0 {
            // bit is nul, up count
            zero_count += 1;
            if zero_count >= 6 {
                // 6 consectuive 0s
                return false;
            }
        }
        else {
            // not 0 bit, reset 0 counter
            zero_count = 0;
        }
    }

    // not advertising AA or 1 hamming distance away from advertising AA
    // TODO uncomment, nrf usbs use advertising AA
    /*
    if aa == 0x8E89BED6 {
        return false;
    }
    for bit_index in 0..32 {
        // flip each bit and check
        // xor bit with 1 is flip, with zero is stay the same.
        if (aa ^ (0b1 << bit_index)) == 0x8E89BED6 {
            return false;
        }
    }
    */


    // not all bytes should be equal
    let mask : u32 = 0xFF;
    let first_byte = aa & mask;
    let mut equal = true;
    for other_byte in 1..4 {
        // Shift next byte to the right and mask it. Check if same.
        if ((aa >> (8*other_byte)) & mask) != first_byte {
            equal = false;
            break;
        }
    }
    if equal {
        return false;
    }


    // Should not have more than 24 transitions
    let mut transitions = 0;
    let mut previous_was_1 = false;
    for bit_index in 0..32 {
        let this_is_1 = aa & (0b1 << bit_index) != 0;
        if bit_index != 0 {
            // xor is one if both were different, otherwise 0
            if this_is_1 ^ previous_was_1 {
                transitions += 1;
                if transitions >= 26 {
                    return false;
                }
            }
        }
        previous_was_1 = this_is_1;
    }

    let mut transitions = 0;
    let mut previous_was_1 = false;
    // Minimum of 2 transitions in 6 most significant bits
    for bit_index in 0..32 {
        let this_is_1 = aa & (0b1 << bit_index) != 0;
        // 6th MSb start at shift 26, start counting one after that
        if bit_index > 26 {
            // xor is one if both were different, otherwise 0
            if this_is_1 ^ previous_was_1 {
                transitions += 1;
            }
        }
        previous_was_1 = this_is_1;
    }
    if transitions < 2 {
        return false;
    }

    // EXTRA FOR CODED PHY
    match phy {
        BlePHY::CodedS2 | BlePHY::CodedS8 => {
            // Shal have at least 3 ones in the least significant 8 bits
            let mut ones = 0;
            for bit_index in 0..32 {
                if bit_index < 8 {
                    if (aa & (0b1 << bit_index)) != 0 {
                        ones += 1;
                    }
                }
            }
            if ones < 3 {
                return false;
            }

            // no more than eleven tranitions in least significant 16 bits
            let mut transitions = 0;
            let mut previous_was_1 = false;
            for bit_index in 0..32 {
                let this_is_1 = aa & (0b1 << bit_index) != 0;
                if bit_index != 0 && bit_index < 16 {
                    // xor is one if both were different, otherwise 0
                    if this_is_1 ^ previous_was_1 {
                        transitions += 1;
                        if transitions >= 11 {
                            return false;
                        }
                    }
                }
                previous_was_1 = this_is_1;
            }

        },
        _ => {}
    }
    

    true
}

// If it is a control pdu as we expect, dont return
// THIS WILL BE OK FOR ENCRYPTED TRAFFIC BECAUSE THE PDU ONLY ENCRYPTS ITS PAYLOAD AND NEVER ITS HEADER
// firstbyte & 0xF3 == 0b1 and second ==0 but I still have to adapt something here I remember because a field he thought would be 0 might not be null anymore because it is used now

// The second byte is the packet length, which should be 0 for data physical (connection event control)packets. We are listening to the controller of the connection, not the peripheral
// Now we have extra field that can be something that does not matter: the CP field for directional BLE detection.
// What we want is LLID = 10 (start of data pdu), Length 0 = controller event start and MD = 0 = last controller packet in event. NESN SN and CP can be what they want. RTU = for future use should always be 0 for now. We can filter extra packets like this.
// So we want 10xx0x00
// Damien does it in reverse however :/ so would in that way would need to be ((first_header_byte & 0b0001_0011) == 0b0000_0001)
// TODO determine reverse or not
#[inline]
fn is_valid_discover_header(first_byte : u8, second_byte : u8) -> bool {
    let ret = (((first_byte & 0b1100_1000) == 0b1000_0000) || ((first_byte & 0b0001_0011) == 0b0000_0001)) && second_byte == 0;
    true
}


#[inline]
pub fn reverse_bits(byte: u8) -> u8 {
    let mut reversed_byte : u8 = 0;
    // Go right to left over original byte, building and shifting the reversed one in the process
    for bit_index in 0..8 {
        // Move to left to make room for new bit on the right (new LSB)
        reversed_byte = reversed_byte << 1;
        // If byte is 1 in its indexed place, set 1 to right/LSB reversed
        if byte & (1 << bit_index) != 0 {
            reversed_byte = reversed_byte | 0b0000_0001;
        }
        else {
            reversed_byte = reversed_byte | 0b0000_0000;
        }
        //reversed_byte |= if byte & (1 << bit_index) != 0 {0b0000_0001} else {0b0000_0000};
    }
    reversed_byte
}
