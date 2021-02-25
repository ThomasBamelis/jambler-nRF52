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
    pub packet_phy: BlePHY,
    /// The system time the packet was caught on in milliseconds.
    pub time: u64,
    /// The time listened on this channel before the packet was caught,
    pub time_on_the_channel: u64,
    /// The rssi
    pub packet_rssi : i8,
    pub packet_crc : u32,
    pub packet : [u8 ; 258],
    /// If there was a response, this will be it (packet, crc, rssi, phy)
    pub response : Option<([u8 ; 258], u32, i8)>,
    pub response_phy : BlePHY,
    pub channel : u8,
}

// TODO IMPORTANT, DO NOT ACCEPT THE REVERSED CRC AT ONCE!!! IT MIGHT HAVE BEEN WRONG BECAUSE OF BIT FLIPS ON AIR! CHECK IT MULTIPLE TIMES! TO REVERSE YOU NEED THE WHOLE PDU + CRC. SO INCLUDING THE HEADER

/// A struct representing the state for harvesting packets of a given access address.
#[derive(Debug)]
pub struct HarvestPackets {
    /// The access address to be listening for.
    access_address: u32,
    /// The PHY the sniffer is listening to
    phy: BlePHY,
    /// The PHY of the slave
    slave_phy : BlePHY,
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
        radio.harvest_packets_quick_config(self.access_address, self.phy, channel, self.crc_init)?;
        radio.receive();

        // set the start time to the new channel
        self.start_time_current_channel = current_time;
        // Remove the master packet seen
        self.previous_master_hal_packet = None;


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
            slave_phy: BlePHY::Uncoded1M,
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


            // set slave phy
            self.slave_phy = config.slave_phy.ok_or(StateError::InvalidConfig("Slave PHY not provided for harvesting packets."))?;

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

        
        // set start time for this channel
        self.start_time_current_channel = parameters.current_time;

        // start listening on channel 0
        self.current_channel = 0;

        // Get the current channel from the channel chain
        let channel : u8 = self.channel_chain[self.current_channel];

        // Config the radio
        parameters.radio.prepare_for_config_change();

        // Config the radio
        parameters.radio.harvest_packets_quick_config(self.access_address, self.phy, channel, self.crc_init)?;
        rprintln!("Init to harvesting for packets: channel {}.", channel);


        // Cache the time we will wait
        self.time_on_one_channel_cache = self.calculate_receiving_interval();

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
            
            // We will be able to reverse the crc either way once we settled on one.

            // only crc change, nothing to report or change
            Ok(None)
        }
    }

    fn stop(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) {
        // the state.rs reset the radio

    }

    /// Will be called when a packet is captured on the configured channel, access address and phy.
    /// The responsibility of this state is to collect the packets with all necessary parameters.
    /// To reduce complexity all decisions are made in the background process.
    ///  
    #[inline]
    fn handle_radio_interrupt(
        &mut self, parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Result<Option<StateReturn>, StateError> {

    

        // Get the packet from the hal
        // radio is responsible for timing out
        let hal_ret = parameters.radio.harvest_packets_busy_wait_slave_response(self.slave_phy);

        // Log timestamp for now
        //TimeStamp::rprintln_normal_with_micros_from_microseconds(parameters.current_time);

        match hal_ret {
            None => {
                // The interrupt fired for another reason, not giving us a packet. 
                // Just return
                rprintln!("Did not result in harvested packet (yet).");
                Ok(None)
            }
            Some(( (master_pdu, master_crc, master_rssi) , slave_response_option)) => {
                // We received a packet and possibly its response
                
                let channel = self.channel_chain[self.current_channel as usize];

                // next channel
                let channel_chain_completed = self.next_channel(parameters.radio, parameters.current_time)?;

                // Construct the return
                let mut sret =  StateReturn::new();

                // Return both the master and the slave
                // TODO change this None, change harvested packet to harvested subevent and change the structure of everything so that jambler has 1 static StateParameters and 1 static Result<Option<StateReturn>, ERr> and provide mutable references in the state function, so that these whole packets do not get pushed on the stack...
                sret.state_message = Some(StateMessage::HarvestedSubevent(HarvestedPacket {
                    access_address: self.access_address,
                    packet_phy: self.phy,
                    time: parameters.current_time,
                    time_on_the_channel: parameters.current_time - self.start_time_current_channel,
                    packet_rssi : master_rssi,
                    packet_crc : master_crc,
                    packet : master_pdu,
                    response : slave_response_option,
                    response_phy : self.slave_phy,
                    channel: channel
                }, None, channel_chain_completed));

                // And request a periodic timer 
                // The interval timer must be reset by this as well!! We do not want an interrupt from the countdown after this!!
                // This is the responsibility of the interval_timer_hal
                sret.timing_requirements = Some(IntervalTimerRequirements::Periodic(self.time_on_one_channel_cache));

                // Reset that we requested for this
                self.request_periodic_timer_on_next_interval_timer_interrupt = false;
                
                Ok(Some(sret))

                
            }
        }
    }

    /// Will get called when we have to change channel and consider this one unused.
    #[inline]
    fn handle_interval_timer_interrupt(
        &mut self, parameters: &mut StateParameters<impl JamBLErHal> 
    ) -> Result<Option<StateReturn>, StateError> {


        TimeStamp::rprintln_normal_with_micros_from_microseconds(parameters.current_time);

        // We will always return something (channel unused or singular packet)
        // So init the return
        let mut ret = StateReturn::new();

        let will_wrap : bool;
        if self.current_channel ==  self.channel_chain.len() - 1 {
            will_wrap = true;
        }
        else {
            will_wrap = false;
        }

        // If we asked a countdown timer because of an interval update, still ask for periodic one
        if self.request_periodic_timer_on_next_interval_timer_interrupt {
            // reset flag
            self.request_periodic_timer_on_next_interval_timer_interrupt = false;
            // Ask for a periodic timer
            ret.timing_requirements = Some(IntervalTimerRequirements::Periodic(self.time_on_one_channel_cache));
        }

        rprintln!("Timeout on channel {}, consider unused", self.channel_chain[self.current_channel]);
        ret.state_message = Some(StateMessage::UnusedChannel(self.channel_chain[self.current_channel], will_wrap));

        // Change channel (don't worry, the handle get a lock on self)
        let channel_chain_completed = self.next_channel(parameters.radio, parameters.current_time)?;

        assert_eq!(channel_chain_completed, will_wrap);

        Ok(Some(ret))
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

