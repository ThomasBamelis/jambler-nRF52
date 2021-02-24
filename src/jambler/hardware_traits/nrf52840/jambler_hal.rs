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

        // TODO fix the write after write bugs ,where you have to use modify after the first write because it will erase it otherwise

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

        let radio = &mut self.radio_peripheral;

        if radio.power.read().power().is_disabled() {
            rprintln!("ERROR: power disabled while harvest packet config change");
            panic!()
        }
        if !radio.state.read().state().is_disabled() {
            rprintln!("ERROR: radio not disabled while harvest packet config change");
            panic!()
        }

        // high frequency clock already done

        // Select reception on 0th of 0-7 possible AAs to listen for
        radio.rxaddresses.write(|w| w.addr0().enabled());
        radio.base0.write(|w| unsafe{w.bits(access_address << 8)});
        radio.prefix0.write(|w| unsafe {w.ap0().bits((access_address >> 24) as u8)} );

        // set packet pointer
        let ptr = self.receive_buffer.as_ptr() as u32;
        radio.packetptr.write(|w| unsafe { w.packetptr().bits(ptr) });

        // Set the frequency to the channel
        let freq = Nrf52840JamBLEr::channel_to_frequency_register_value(channel);
        radio.frequency.write(|w| unsafe {  w.frequency().bits(freq)}); 

        // Set crc
        match crc_init {
            Some(crc_init) => {
                radio.crcinit.write(|w| unsafe{w.crcinit().bits(crc_init)});
            }
            None => {
                radio.crcinit.reset();
            }
        }
        radio.crccnf.write(|w| { w.len().three().skipaddr().skip() });
        radio.crcpoly.write(|w| unsafe {w.crcpoly().bits(0b00000001_00000000_00000110_01011011)});

        // Set datawhitening seed
        radio.datawhiteiv.write(|w| unsafe{w.datawhiteiv().bits(channel)});

        // pcnf1
        radio.pcnf1.write(|w| unsafe { w .balen().bits(3).statlen().bits(0).maxlen().bits(255).endian().little().whiteen().set_bit() });


        // pcnf0
        // Always receive a third byte as header
        // On receive, check if 3-byte header flag is set and read in the crc as you should
        // S0 will be automatically included in the header
        radio.pcnf0.write(|w| unsafe{w.lflen().bits(8).s0len().bit(true).s1len().bits(8).crcinc().exclude()});


        // PHY dependend
        // Set the PHY mode and the corresponding preamble and cilen and termlen
        match phy {
            BlePHY::Uncoded1M => {
                radio.mode.write(|w| w.mode().ble_1mbit());
                radio.modecnf0.write(|w| w.ru().default().dtx().b1());
                radio.pcnf0.modify(|_, w| unsafe{w.plen()._8bit().cilen().bits(0).termlen().bits(0)});
            },
            BlePHY::Uncoded2M => {
                radio.mode.write(|w| w.mode().ble_2mbit());
                radio.modecnf0.write(|w| w.ru().default().dtx().b1());
                radio.pcnf0.modify(|_, w| unsafe{w.plen()._16bit().cilen().bits(0).termlen().bits(0)});
            },
            BlePHY::CodedS2 => {
                radio.mode.write(|w| w.mode().ble_lr500kbit());
                radio.modecnf0.write(|w| w.ru().default().dtx().center());
                radio.pcnf0.modify(|_, w| unsafe{w.plen().long_range().cilen().bits(2).termlen().bits(3)});
            },
            BlePHY::CodedS8 => {
                radio.mode.write(|w| w.mode().ble_lr125kbit());
                radio.modecnf0.write(|w| w.ru().default().dtx().center());
                radio.pcnf0.modify(|_, w| unsafe{w.plen().long_range().cilen().bits(2).termlen().bits(3)});
            }
        }

        
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


        Ok(())
    }

    /// Returns Some if the packet just received was the first packet in the connection event and the boolean inside is true if the crc check was passed, false otherwise.
    /// Otherwise None.
    fn handle_harvest_packets_radio_interrupt(&mut self) -> Option<HalHarvestedPacket> {


        let radio = &mut self.radio_peripheral;

        // Reset the end event, otherwise the interrupt will keep on firing
        radio.events_end.reset();

        // Get the first header byte
        let first_header_byte = unsafe { core::ptr::read_volatile(&self.receive_buffer[0]) };
        // Get received length
        let payload_len : u8 = unsafe { core::ptr::read_volatile(&self.receive_buffer[1]) };

        // Get the rssi
        let rssi : i8 = - ( (radio.rssisample.read().bits() as u8) as i8);

        // Check if the header is 3 bytes
        // Remember, they show this in the reversed way in the specification (the fields of a byte, but the bits of a field are normal e.g. LLID 2 = 0bxxxx_xx10)
        let cp_bit_set = (first_header_byte & 0b0010_0000) != 0;

        // Extract the crc and if was correct
        let received_crc : u32;
        let pdu_length : u16;
        if cp_bit_set {
            // 3 byte header
            received_crc = radio.rxcrc.read().bits();
            // Full PDU length = header length + payload length
            pdu_length = 3 + (payload_len as u16);
        }
        else {
            // Get the crc byte which was interpreted as the last byte of the packet
            // This is an index (start from 0)
            let misplaced_crc_byte : u8 = unsafe { core::ptr::read_volatile(&self.receive_buffer[(1 + payload_len + 1) as usize]) };
            // Get malformed crc the radio thaught it was due to misalignment
            let malformed_rxcrc = radio.rxcrc.read().bits();
            // Reconstruct the actual crc
            received_crc = (reverse_bits(misplaced_crc_byte) as u32) << 16 | (malformed_rxcrc >> 8);
            // Full PDU length = header length + payload length
            pdu_length = 2 + (payload_len as u16);
        }

        // Check if the crc was correct
        // TODO this will be garbage if the crcinit has not been initialised
        let crc_ok : bool;
        if cp_bit_set {
            // If this was a 3-byte header, just read it from the radio
            crc_ok = radio.crcstatus.read().crcstatus().is_crcok();
        }
        else {
            // Check it ourselves, get it from the buffer
            let crc_init = radio.crcinit.read().bits();
            // Calculate and determine if it was ok
            let calculated_crc = calculate_crc(crc_init, &self.receive_buffer, pdu_length);
            if calculated_crc == received_crc {
                crc_ok = true;
            }
            else {
                crc_ok = false;
            }
        }

        // Calculate the reverse crc init. Delete if it causes timing issues
        let reverse_calculated_crc_init = reverse_calculate_crc_init(received_crc, &self.receive_buffer, pdu_length);

        // Log 
        //rprintln!("Received packet with payload length {} and 3-byte header <-> {}\nS0 0b{:08b}\nLen 0b{:08b}\nReceived crc 0x{:06X}\nCrc ok {}\nReversed crc init 0x{:06X}\nRSSI {}", payload_len, cp_bit_set, first_header_byte, payload_len, received_crc, crc_ok, reverse_calculated_crc_init, rssi);

        // TODO Keep this simple and let the background worker who will have more information (multiple chips) make the decision on what is an anchorpoint and what not, just give the 2 header bytes

        // return it
        // Always return some, we have configured the radio to only receive interrupts on packet reception.
        Some(HalHarvestedPacket {
            crc_ok : crc_ok,
            crc_init : reverse_calculated_crc_init,
            rssi,
            first_header_byte : first_header_byte,
            second_header_byte : payload_len,
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
/// 
/// From the BLE specification:
/// All bits shal be processed in transmitted (on-air) order
/// starting from the least significant bit.
/// 
/// See figure 3.4 (CRC circuit) on page 2924. 
/// What we do is that figure, but reverse the arrows, the crc value in it at the start.
/// Because position 0 gives us x^24 and we know data in (over the air pdu in reverse) we can deduce what position 23 was.
/// Doing this for the whole PDU leaves us with the crc_init value which was originally in the register.
pub fn reverse_calculate_crc_init(received_crc_value : u32, pdu: & [u8], pdu_length : u16) -> u32 {


    let mut state : u32 = reverse_bits_u32(received_crc_value) >> 8;
	let lfsr_mask: u32 = 0xb4c000;

    // loop over the pdu bits (as sent over the air) in reverse
    // The first processed bit is the 0b1xxx_xxxx bit of the byte at index pdu_length of the given pdu
	for byte_number in (0..pdu_length).rev() {
		let current_byte : u8 = pdu[byte_number as usize];
		for bit_position in (0..8).rev() {
            // Pop position 0 = x^24
			let old_position_0 : u8 = (state >> 23) as u8;
            // Shift the register to the left (reversed arrows) and mask the u32 to 24 bits 
			state = (state << 1) & 0xffffff;
            // Get the data in bit
            let data_in = (current_byte >> bit_position) & 1; 
            // xor x^24 with data in, giving us position 23
            // we shifted state to the left, so this will be 0, so or |= will set this to position 23 we want
			state |= (old_position_0 ^ data_in) as u32;
            // In the position followed by a XOR, there sits now the result value of that XOR with x^24 instead of what it is supposed to be.
            // Because XORing twice with the same gives the original, just XOR those position with x^24. So XOR with a mask of them if x^24 was 1 (XOR 0 does nothing)
			if old_position_0 != 0 {
				state ^= lfsr_mask;
            }
		}
	}

    // Position 0 is the LSB of the init value, 23 the MSB (p2924 specifications)
    // So reverse it into a result u32
	let mut ret : u32 = 0;
    // Go from CRC_init most significant to least = pos23->pos0
	for i in 0..24 {
		ret |= ((state >> i) & 1) << (23 - i);
    }

	return ret;
}

pub fn calculate_crc(crc_init : u32, pdu: & [u8], pdu_length : u16) -> u32 {

    // put crc_init in state, MSB to LSB (MSB right)
    
    let mut state : u32 = 0;
    for i in 0..24 {
		state |= ((crc_init >> i) & 1) << (23 - i);
    }
	let lfsr_mask: u32 = 0b0101_1010_0110_0000_0000_0000;

    // loop over the pdu bits (as sent over the air) 
    // The first processed bis it the 0bxxxx_xxx1 bit of the byte at index 0 of the given pdu
	for byte_number in (0..pdu_length) {
		let current_byte : u8 = pdu[byte_number as usize];
		for bit_position in (0..8) {
            // Pop position 23 x^24
			let old_position_23 : u8 = (state & 1) as u8;
            // Shift the register to the right  
			state = state >> 1 ;
            // Get the data in bit
            let data_in = (current_byte >> bit_position) & 1; 
            // calculate x^24 = new position 0 and put it in 24th bit
            let new_position_0 = (old_position_23 ^ data_in) as u32;
			state |= new_position_0 << 23;
            // if the new position is not 0, xor the register pointed to by a xor with 1
			if new_position_0 != 0 {
				state ^= lfsr_mask;
            }
		}
	}

    // Position 0 is the LSB of the init value, 23 the MSB (p2924 specifications)
    // So reverse it into a result u32
	//let mut ret : u32 = 0;
    // Go from CRC_init most significant to least = pos23->pos0
	//for i in 0..24 {
	//	ret |= ((state >> i) & 1) << (23 - i);
    //}

	return reverse_bits_u32(state) >> 8;
}


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

pub fn reverse_bits_u32(byte: u32) -> u32 {
    let mut reversed_byte : u32 = 0;
    // Go right to left over original byte, building and shifting the reversed one in the process
    for bit_index in 0..32 {
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

