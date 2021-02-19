use crate::jambler::JamBLErHalError;
use crate::jambler::HalHarvestedPacket;
use crate::jambler::state::StateMessage;
use crate::jambler::state::DiscoveredAccessAddress;
use crate::jambler::StateReturn;
use super::StateParameters;
use crate::jambler::state::IntervalTimerRequirements;
use crate::jambler::state::StateConfig;
use crate::jambler::state::StateError;
use crate::jambler::JamBLErState;
use heapless::{consts::*, spsc::Queue, Vec};

use super::super::util::TimeStamp;

use super::super::{BlePHY, JamBLErHal};
use super::{ JammerState};

use rtt_target::rprintln;

/// A struct holding all relevant information regarding a harvested packet
/// necessary for recovering the connection parameters.
#[derive(Clone,Debug)]
pub struct HarvestedPacket {
    /// The access address of the packet.
    pub access_address: u32,
    /// The PHY the packet was caught on.
    pub phy: BlePHY,
    /// The system time the packet was caught on in milliseconds.
    pub time: u64,
    /// Some if crc has been enabled by providing a Some(crc_init) value in the latest config_harvest_packets. True if the crc check was successfull, false if it wasn't.
    pub crc_ok : Option<bool>,
    /// Some if instructed to reverse calculate the crc init and able to do so. 
    /// Then it contains the reversed crc for this packet as it has been received (might have been received with errors).
    pub crc_init : Option<u32>,
    /// The rssi, why not
    pub rssi : i8,
    // Whether it is an anchor point
    pub is_anchor_point : bool,
}

// TODO IMPORTANT, DO NOT ACCEPT THE REVERSED CRC AT ONCE!!! IT MIGHT HAVE BEEN WRONG BECAUSE OF BIT FLIPS ON AIR! CHECK IT MULTIPLE TIMES! TO REVERSE YOU NEED THE WHOLE PDU + CRC. SO INCLUDING THE HEADER

/// A struct representing the state for harvesting packets of a given access address.
#[derive(Debug)]
pub struct HarvestPackets {
    /// The access address to be listening for.
    access_address: u32,
    /// The PHY the sniffer is listening to
    phy: BlePHY,
    /// The channels the listener will listen for.
    /// Must not be empty and all elements must be between 0 and 37.
    channel_chain: Vec<u8,U64>,
    /// The current minimum connection interval in microseconds.
    /// Must be a multiple of 1.25 ms (1250 micros), between 7.5 milliseconds and 4 seconds.
    current_min_conn_interval: u32,
    /// The number of intervals the sniffer is supposed to listen on 1 channel before changing channels. 
    number_of_intervals: u32,
    // Current channel (index into channel chain).
    // Must always be a legal index for channel chain.
    current_channel: usize,
    /// The crc init value, used for the connection, if recovered already.
    /// 24-bit
    crc_init: Option<u32>,
    /// The start time of listinening on the current channel 
    start_time_current_channel: u64,
    /// The clock drift in ppm of the interval timer.
    interval_timer_ppm : u32,
    /// The clock drift in ppm of the long term timer which provides the current_time in the parameters.
    long_term_timer_ppm : u32,
    /// an indicator necessary for flagging the interval timer interrupt we have to change to timer to periodic, as we will have used a countdown to reuse the time we were already listening on that channel!
    request_periodic_timer_on_next_interval_timer_interrupt : bool,
    /// a cache so we do not need to recalculate with floats on every interval timer interrupt
    time_on_one_channel_cache : u32,
    /// Is some if we received a master packet on this channel already.
    /// Used to receive a connection event with 2 empty pdus the conclude the first one was an anchor point.
    previous_master_hal_packet : Option<HarvestedPacket>,
}

impl HarvestPackets {

    /// Calculates the worst case interval in microseconds to wait for with the window widening specified in the specification.
    /// See specification page 2930
    #[inline]
    fn calculate_receiving_interval(&self) -> u32 {
        // The theoretical perfect case
        let mut total_time =  self.current_min_conn_interval * self.number_of_intervals;

        // Add other side worst case (500ppm sleep clock accuracy) clock drift
        total_time += (total_time as f32 * (500 as f32 / 1_000_000 as f32)) as u32 + 1;

        // Add the instant tolerance worst case (2 for active, 16 for sleep)
        total_time += 16;

        // Add range delay = 2*D*4 nanoseconds with D the distance in meters
        // This is 24 microseconds for 3km distance
        total_time += 24;

        // Add my own worst case interval timer clock accuracy
        total_time += (total_time as f32 * (self.interval_timer_ppm as f32 / 1_000_000 as f32)) as u32 + 1;
        
        total_time
    }

    /// Calculates the minimum anchor distance as specified in the comments of the radio interrupt handler in microseconds.
    #[inline]
    fn calculate_minimum_anchor_distance(&self) -> u32 {
        // T_IFS + 2 (for instant)
        let mut minimum_distance = 150 + 2;

        // Add other side worst case (50ppm active clock accuracy) clock drift
        minimum_distance += (minimum_distance as f32 * (50 as f32 / 1_000_000 as f32)) as u32 + 1;

        // Add range delay = 2*D*4 nanoseconds with D the distance in meters
        // This is 24 microseconds for 3km distance
        minimum_distance += 24;

        // Add my own worst case long term timer clock accuracy
        minimum_distance += (minimum_distance as f32 * (self.long_term_timer_ppm as f32 / 1_000_000 as f32)) as u32 + 1;
        
        minimum_distance
    }

    /// Lets the radio listen on the next channel.
    /// Return true if the channel chain was completed and wrapped.
    #[inline]
    fn next_channel(&mut self, radio: &mut impl JamBLErHal, current_time : u64) -> Result<bool,JamBLErHalError> {

        let old_channel = self.channel_chain[self.current_channel];

        // Change channel
        // Could do modulo, but I think it is very slow so I do it this way
        self.current_channel += 1;
        let wrapped;
        // Wrap around chain when necessary
        if !(self.current_channel < self.channel_chain.len()) {
            self.current_channel = 0;
            wrapped = true;
        }
        else {
            wrapped = false;
        }

        // reconfigure the radio channel
        let channel = self.channel_chain[self.current_channel];

        radio.prepare_for_config_change();
        // Config the radio
        radio.config_harvest_packets(self.access_address, self.phy, channel, self.crc_init)?;
        radio.receive();
        // set the start time to the new channel
        self.start_time_current_channel = current_time;


        rprintln!("Changing channel {}->{}.", old_channel, channel);
        

        Ok(wrapped)
    }
}

impl JammerState for HarvestPackets {

    /// Creates a dummy harvestPackets state
    fn new() -> HarvestPackets {
        HarvestPackets {
            // Dummy address is the advertising access address.
            access_address : 0x8E89BED6,
            phy: BlePHY::Uncoded1M,
            channel_chain: Vec::new(),
            current_min_conn_interval: 4_000_000,
            number_of_intervals: 100,
            current_channel: 0,
            crc_init: None,
            start_time_current_channel : 0,
            interval_timer_ppm : 500,
            long_term_timer_ppm : 500,
            request_periodic_timer_on_next_interval_timer_interrupt: false,
            time_on_one_channel_cache: 0, // invalid value
            previous_master_hal_packet: None,
        }

    }

    /// Returns an error if a required config parameter was missing.
    fn config(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) -> Result<(), StateError> {
        if let Some(ref config) = parameters.config {

            // set access address
            self.access_address = config.access_address.ok_or(StateError::InvalidConfig("Access address not provided for harvesting packets."))?;

            // set phy
            self.phy = config.phy.ok_or(StateError::InvalidConfig("PHY not provided for harvesting packets."))?;

            // set channel chain
            self.channel_chain = config.channel_chain.clone().ok_or(StateError::InvalidConfig("Channels not provided for harvesting packets."))?;

            // set interval
            self.current_min_conn_interval = config.interval.ok_or(StateError::InvalidConfig("Interval not provided for harvesting packets."))?;

            // set number of intervals
            self.number_of_intervals = config.number_of_intervals.ok_or(StateError::InvalidConfig("Number of intervals not provided for harvesting packets."))?;

            // if a crcInit is given, set it
            // Can just copy because is option as well
            self.crc_init = config.crc_init;


            // set interval timer ppm
            self.interval_timer_ppm = config.interval_timer_ppm.ok_or(StateError::InvalidConfig("Interval timer ppm not provided for harvesting packets."))?;

            // set interval timer ppm
            self.long_term_timer_ppm = config.long_term_timer_ppm.ok_or(StateError::InvalidConfig("Long term timer ppm not provided for harvesting packets."))?;
            

            // check if channel chain is not empty
            // Because of the way it is constructed there will be 64 elements at most, don't check upper bound.
            if self.channel_chain.is_empty() {
                return Err(StateError::InvalidConfig("Channel chain empty for harvesting packets."));
            }



            self.current_channel = 0;
            // Check if all channels are legal
            while self.current_channel < self.channel_chain.len()  {
                let channel : u8 = self.channel_chain[self.current_channel];
                if channel > 36 {
                return Err(StateError::InvalidConfig("Illegal channel in channel chain for harvesting packets."));
                }
                self.current_channel += 1;
            }

            // will always be legal value
            self.current_channel = 0;

            // check if interval is at least 7.5 milliseconds
            // and not larger than 4 seconds and a multiple of 1.25 ms
            // (the minimum for conInterval)
            if self.current_min_conn_interval < 7_500 {
                return Err(StateError::InvalidConfig("Interval for discovering AAs was shorter than the minimum connection interval (7.5 ms)."));
            }
            else if self.current_min_conn_interval > 4_000_000 {
                return Err(StateError::InvalidConfig("Interval for discovering AAs was longer than the maximum connection interval (4s)."));
            }
            else if self.current_min_conn_interval % 1_250 != 0 {
                return Err(StateError::InvalidConfig("Interval for discovering AAs was not a multiple of 1.25 milliseconds."));
            }

            // Cache the time we will wait
            self.time_on_one_channel_cache = self.calculate_receiving_interval();

            // Changed channel, reset the previous hal harvested master packet
            self.previous_master_hal_packet = None;

            // Everything was ok and is set
            Ok(())
        }
        else {
            Err(StateError::MissingConfig(
                "Config necessary for harvesting packets."
            ))
        }

    }

    /// Functions as a reset + start!
    /// Configures the radio to be ready for listening on the first channel of the channel chain.
    /// Assumes the radio has been correctly configured.
    fn initialise(
        &mut self,
        parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Result<Option<StateReturn>, StateError> {

        // This is beginning, reset the reuse flag to be sure
        self.request_periodic_timer_on_next_interval_timer_interrupt = false;
        
        // set start time for this channel
        self.start_time_current_channel = parameters.current_time;

        // start listening on channel 0
        self.current_channel = 0;

        // Get the current channel from the channel chain
        let channel : u8 = self.channel_chain[self.current_channel];

        // Config the radio
        parameters.radio.prepare_for_config_change();

        // Config the radio
        parameters.radio.config_harvest_packets(self.access_address, self.phy, channel, self.crc_init)?;
        rprintln!("Init to harvesting for packets: channel {}.", channel);


        let mut ret = StateReturn::new();
        ret.timing_requirements = Some(IntervalTimerRequirements::Periodic(self.time_on_one_channel_cache));
        Ok(Some(ret))
    }

    fn launch(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) {
        TimeStamp::rprint_normal_with_micros_from_microseconds(parameters.current_time);
        rprintln!("Launched harvesting for packets: \n{:?}.", &self);

        // launch the radio
        parameters.radio.receive();
    }

    /// Used for updating the state.
    /// 
    /// Harvesting packets is able to update the following parameters:
    ///     - current_min_conn_interval (interval)
    ///     - crc_init
    /// 
    /// An access address, phy change, channel chain would require starting from scratch anyway.
    /// The ppm, number of intervals etc will not be changed either, just restart.
    fn update_state(
        &mut self, parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Result<Option<StateReturn>, StateError> {


        // remember the current channel index
        let cur_chan = self.current_channel;


        let mut interval_change;

        // TODO channel_chain update. For multiple devices, after this one has done its job so he can help with more unlucky ones which had a lot of unused channels. However, maybe just let this state finish? You will always rely on outside jambler sources to transition which is basically a new task. I dunno, see later


        match parameters.config {
            Some(ref mut c) => {

                // assign necessary but unupdatable parameters

                // return error if any of these are not None
                if !(c.access_address.is_none() &&
                        c.phy.is_none() &&
                        c.number_of_intervals.is_none() &&
                        c.interval_timer_ppm.is_none() &&
                        c.channel_chain.is_none()
                    ) {
                    return Err(StateError::InvalidConfig("Illegal update parameters provided for harvesting packets update"));
                }
                c.access_address = Some(self.access_address);
                c.phy = Some(self.phy);
                c.number_of_intervals = Some(self.number_of_intervals);
                c.interval_timer_ppm = Some(self.interval_timer_ppm);
                c.channel_chain = Some(self.channel_chain.clone());

                // Check for interval
                match c.interval {
                    None => {
                        // No new interval given, give it current one
                        c.interval = Some(self.current_min_conn_interval);
                        interval_change = false;
                    }
                    Some(new_interval) => {
                        if new_interval >= self.current_min_conn_interval {
                            return Err(StateError::InvalidConfig("Interval update for harvesting packets update was not shorter"));
                        }
                        interval_change = true;
                    }
                };

                // Check for crc init change
                match c.crc_init {
                    None => {
                        // No new phy given, give it current one
                        // Do not wrap in option, it already is one
                        c.crc_init = self.crc_init;
                    }
                    Some(_) => {}
                };
                

            },
            None => {
                return Err(StateError::InvalidConfig("No config provided for harvesting packets update"));
            }
        }

        // set the new configuration to this local struct, validating them as well
        self.config(parameters)?;

        // restore channel index from config
        self.current_channel = cur_chan;

        
        TimeStamp::rprint_normal_with_micros_from_microseconds(parameters.current_time);
        rprintln!("Harvesting packets state update: interval change {}", interval_change);

        if interval_change {
            // There was an interval change
            // refresh the cache
            self.time_on_one_channel_cache = self.calculate_receiving_interval();
            let listening_time_on_this_channel =  (parameters.current_time - self.start_time_current_channel) as u32;

            if self.time_on_one_channel_cache <= listening_time_on_this_channel {
                // new time to change is shorter than the time already on this channel
                // Change the channel with new periodic timer
                
                // Change channel
                self.next_channel(parameters.radio, parameters.current_time)?;

                let mut ret = StateReturn::new();
                ret.timing_requirements = Some(IntervalTimerRequirements::Periodic(self.time_on_one_channel_cache));
                Ok(Some(ret))

            }
            else {
                // new time to change is longer than the time we are already listening on this channel
                // lets reuse that!
                // Countdown until the end of this cycle

                // Set flag so that interval timer will now it has to set the timer back to periodic
                self.request_periodic_timer_on_next_interval_timer_interrupt = true;

                
                let mut ret = StateReturn::new();

                // ask for a countdown timer until then
                ret.timing_requirements = Some(IntervalTimerRequirements::Countdown(self.time_on_one_channel_cache - listening_time_on_this_channel));
                Ok(Some(ret))
            }
        }
        else {

            // reconfigure the radio still, will have been a crc_init update
            let channel = self.channel_chain[self.current_channel];

            // Config the radio
            parameters.radio.prepare_for_config_change();
            parameters.radio.config_harvest_packets(self.access_address, self.phy, channel, self.crc_init)?;
            parameters.radio.receive();

            // only crc change, nothing to report or change
            Ok(None)
        }
    }

    fn stop(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) {
        // the state.rs reset the radio

    }

    /// Here I deviate from Damien. His method seems quite error prone and not robust and must fail quite often I suspect. 
    /// My method should identify more anchor points (events on which data is sent) and should send only true anchor points (not slave packets)
    /// 
    /// For recovering the CRC: any packet will do as long as we can capture the entire packet (PDU) and the CRC.
    /// then we can reverse it.
    /// 
    /// We want to intercept anchor points of the connection:
    /// the first packet sent by the master on the channel.
    /// PROBLEM: no way to determine for sure any random packet is the anchor (first packet sent by master in a connection event) from the packet contents alone. Capturing the slave packet or subsequent might mess with our connInterval guess.
    /// What we know:
    ///     - A slave packet ALWAYS comes 150 microseconds after a master packet
    ///     - A master packet comes 150 microseconds after a slave packet IF AND ONLY IF it is a continuation (not first subevent) of the connection event = IT IS NOT THE ANCHOR POINT
    ///     - An anchor point comes connInterval (>= 7,5 milliseconds) after the previous anchor point. However, an anchor point might be followed by many other packets.
    ///     - A connection event will be closed at least T_IFS before the anchor point of the next event
    ///
    /// CONCLUSION: if a packet on a given channel, phy and access address is sent more than MIN_DIFFERENCE = T_IFS + 2 + T_Range (plus accounted for 50PPM sender clock drift + own clock drift) AFTER the previous packet on the given channel, phy and access address it is an anchor point for that connection.
    /// 
    /// Pitfalls: 
    ///     - We might miss an anchor point for which the previous connection event ended MIN_DIFFERENCE + any extra buffertime before.
    ///     - If we miss the previous packet in a connection event and wrongly identify the next packet as an anchor point (malformed access address -> (most likely malformed at other receiver as well, what happens then? is it a problem), not in range -> very unlikely 1 is not and the rest is because so close after one another).
    /// 
    /// So keep the time when the previous packet arrived and do not accept new anchorpoints closer than MIN_DIFFERENCE + a sizeable buffer (I think getting close like that is quite rare). 
    /// Set to start listening on channel when you first start to listen, should you start listening in the middle of a connection event.
    /// 
    /// 
    /// FOR channel map: any packet (preferably CRC checked to prevent false positives) will do
    /// 
    /// For processing, maybe immediately switch to new channel (for which you dont know channel map yet) if one is found. This has no effect on the CRC and hopinterval recovery really and will make you converge to the unused channels quicker (where you will wait for a 95% sure not in use)
    /// 
    /// 
    /// 
    /// TODO above is not true, subevents can be sent on a different channel, I think 2 empty PDUs are the only guarantee of a connection event
    #[inline]
    fn handle_radio_interrupt(
        &mut self, parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Result<Option<StateReturn>, StateError> {
        TimeStamp::rprintln_normal_with_micros_from_microseconds(parameters.current_time);

        // get the packet from the radio hal
        // reverse calculate crc init if we don't have it yet
        let reverse_crc : bool ;
        match self.crc_init.as_ref() {
            Some(_) => {reverse_crc = false}
            None => {reverse_crc = true}
        }
        let hal_ret = parameters.radio.handle_harvest_packets_radio_interrupt(reverse_crc);

        match hal_ret {
            Some(hret) => {
                match self.previous_master_hal_packet.as_mut() {
                    None => {
                        // first packet on this channel, phy and address
                        // might be a connection event anchor point

                        // Check if we have been on this channel longer than the minimum distance required for the packet to be a packet from the master.
                        let min_ancher_time = self.calculate_minimum_anchor_distance();
                        let is_master_packet = (min_ancher_time as u64) < (parameters.current_time - self.start_time_current_channel);

                        // check if first header byte is empty pdu header.
                        // A whole connection event is often 2 empty PDUs to keep the connection alive. It is the only way to know for sure we have an anchor point.
                        // TODO: the ancher timer might be enough if we count on the connection event not lasting longer than 1.25 milliseconds (will be same as anchor point when done modulo 1.25 as connInterval must be multiple of this). This is possible but not likely. If we have a busy line without empty PDU keep alive we will never find an anchor point
                        // NESN, SN and CP can be what they want
                        // MD must be 0 (no more data), LLID must be 01, RFU must be 0
                        // So 0bxxx0_xx01
                        // mask = 0b0001_0011 (reverse picture p2892)
                        // and check if second header byte (lenght) 0
                        // TODO some control PDU might only be sent by the master maybe, so  might be worth checking?
                        let is_empty_pdu = (hret.first_header_byte & 0b0001_0011) ==  0b0000_0001
                            && hret.second_header_byte == 0;

                        let could_be_anchor_point = is_master_packet && is_empty_pdu;

                        let harvested_packet = HarvestedPacket {
                            access_address: self.access_address,
                            phy: self.phy,
                            time: parameters.current_time,
                            crc_ok : hret.crc_ok,
                            crc_init : hret.crc_init,
                            rssi : hret.rssi,
                            is_anchor_point : false, // Set to false for now
                        };

                        // The return value we will build here
                        let mut sret =  StateReturn::new();

                        if could_be_anchor_point {
                            // if candidate, set countdown timer and prev master, so we have a limited time window

                            // remember the harvested packet
                            self.previous_master_hal_packet = Some(harvested_packet);

                            // Set the countdown timer to the minimum anchor distance (this will be what )
                            // We have no packet to report back yet (wait for next slave to confirm this was an anchor point)
                            rprintln!("Potential anchor point found, wait for slave response before moving on to next channel to be sure.");
                            sret.timing_requirements = Some(IntervalTimerRequirements::Countdown(min_ancher_time));

                        }
                        else {
                            // if not a candidate, return not anchor point and change channel
                            // If the countdown expires before a next packet is reached, prev_master will be set to none
                            // Just change channel, this is not a candidate and the next anchor point might be on any channel. 

                            rprintln!("Harvested packet for which we could never be sure it was an anchorpoint.");
                            
                            // next channel
                            let channel_chain_completed = self.next_channel(parameters.radio, parameters.current_time)?;

                            // This way we explore the channels faster
                            sret.state_message = Some(StateMessage::HarvestedPacket(harvested_packet, channel_chain_completed));

                        }
                        Ok(Some(sret))
                    }
                    Some(prev_master) => {
                        // Received another packet before channel change
                        // check if also empty pdu
                        let is_empty_pdu = (hret.first_header_byte & 0b0001_0011) ==  0b0000_0001
                            && hret.second_header_byte == 0;

                        // If this is an empty pdu as well and received T_IFS after the previous packet, the previous empty packet was an anchorpoint
                        if is_empty_pdu {
                            prev_master.is_anchor_point = true;
                            rprintln!("Previous master was anchor point.");
                        }
                        else {
                            rprintln!("Previous master was not anchor point for sure.");
                        }

                        // for borrowing rules
                        let retp = prev_master.clone();

                        // next channel
                        let channel_chain_completed = self.next_channel(parameters.radio, parameters.current_time)?;

                        // Return it
                        let mut sret =  StateReturn::new();
                        sret.state_message = Some(StateMessage::HarvestedPacket(retp, channel_chain_completed));
                        Ok(Some(sret))

                    }
                }
            }
            None => {
                rprintln!("Did not result in harvested packet (yet).");
                Ok(None)
            }
        }
    }

    /// Interval timer interrupt for going to the next channel in line
    #[inline]
    fn handle_interval_timer_interrupt(
        &mut self, parameters: &mut StateParameters<impl JamBLErHal> 
    ) -> Result<Option<StateReturn>, StateError> {

        TimeStamp::rprintln_normal_with_micros_from_microseconds(parameters.current_time);
        let unused_channel = self.channel_chain[self.current_channel];
        rprintln!("Timeout on channel {}, consider unused", unused_channel);

        
        // next channel
        let channel_chain_completed = self.next_channel(parameters.radio, parameters.current_time)?;


        let state_message = StateMessage::UnusedChannel(unused_channel, channel_chain_completed);

        // if we had a countdown timer because of an interval change or anchor point packet candidate
        // return a periodic timer with the desired interval
        if self.request_periodic_timer_on_next_interval_timer_interrupt {
            // reset flag
            self.request_periodic_timer_on_next_interval_timer_interrupt = false;

            // ask for a periodic timer
            let mut ret = StateReturn::new();
            ret.timing_requirements = Some(IntervalTimerRequirements::Periodic(self.time_on_one_channel_cache));
            // If there was a previous master packet on this channel, return it, otherwise report unused
            match self.previous_master_hal_packet.as_ref() {
                Some(prev) => {
                    // will have its anchor point set to false
                    ret.state_message = Some(StateMessage::HarvestedPacket(prev.clone(), channel_chain_completed));
                }
                None => {
                    // report back unused channel
                    ret.state_message = Some(state_message);
                }
            }
            Ok(Some(ret))
        }
        else {
            // already on the periodic timer, no change in timer requirements
            // Report back unused channel
            let mut ret = StateReturn::new();
            ret.state_message = Some(state_message);
            Ok(Some(ret))
        }
    }

    /// Is it valid to go from the self state to the new state.
    /// self -> new_state valid?
    /// Can only go to idle or start harvesting patterns.
    fn is_valid_transition_to(&mut self, new_state: &JamBLErState) -> Result<(), StateError> {
        match new_state {
            // TODO allow for transition to TestingParameters
            JamBLErState::Idle => {
                Ok(())
            },
            _=> {Err(StateError::InvalidStateTransition(
                "Can only go to idle state or start testing parameters after harvesting packets",
            ))}
        }
    }

    /// Is it valid to go to the self state from the old_state
    /// new_state -> self valid?
    fn is_valid_transition_from(&mut self, old_state: &JamBLErState) -> Result<(), StateError> {
        match old_state {
            JamBLErState::Idle => Ok(()),
            _=> {Err(StateError::InvalidStateTransition(
                "Can start harvesting packets from the Idle state",
            ))}
        }
    }
}