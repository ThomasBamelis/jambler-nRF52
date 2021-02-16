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

/// Struct used to hold state for sniffing access adresses on data channels.
pub struct DiscoverAas {
    /// Cache for adresses already seen.
    /// A queue holding the access address as an unsigned 32 bit.
    aa_cache: Queue<u32, U255, u8>,
    /// The PHY the sniffer is listening to
    phy: BlePHY,
    /// The channels the listener will listen for.
    /// Must not be empty and all elements must be between 0 and 37.
    channel_chain: Vec<u8,U64>,
    /// Time when started to listen on this channel in microseconds.
    /// Must be longer than 1.25 ms (1250 micros).
    interval: u32,
    // Current channel (index into channel chain).
    // Must always be a legal index for channel chain.
    current_channel: usize,
}

impl DiscoverAas {

}


impl JammerState for DiscoverAas {

    /// Creating a new to basically allocate place for this in the state store
    /// without exposing the struct fields.
    fn new() -> DiscoverAas {
        DiscoverAas {
            aa_cache: Queue::u8(),
            phy: BlePHY::Uncoded1M,
            channel_chain: Vec::new(),
            interval: 0,
            current_channel : 0,
        }
    }

    /// Configure the parameters for this state.
    /// Sets the PHY, channels and interval to be snooping for 
    fn config(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) -> Result<(), StateError> {
        if let Some(ref config) = parameters.config {

            // set phy
            self.phy = config.phy.ok_or(StateError::InvalidConfig("PHY not provided for discovering AAs"))?;
            // set channel chain
            self.channel_chain = config.channel_chain.clone().ok_or(StateError::InvalidConfig("Channels not provided for discovering AAs"))?;
            // set interval
            self.interval = config.interval.ok_or(StateError::InvalidConfig("Interval not provided for discovering AAs"))?;

            // check if channel chain is not empty
            // Because of the way it is constructed there will be 64 elements at most, don't check upper bound.
            if self.channel_chain.is_empty() {
                return Err(StateError::InvalidConfig("Channel chain empty for discovering AAs"));
            }


            self.current_channel = 0;
            // Check if all channels are legal
            while self.current_channel < self.channel_chain.len()  {
                let channel : u8 = self.channel_chain[self.current_channel];
                if channel < 0 || channel > 36 {
                return Err(StateError::InvalidConfig("Channel chain empty for discovering AAs"));
                }
                self.current_channel += 1;
            }

            // will always be legal value
            self.current_channel = 0;

            // check if interval is at least 1.25 milliseconds
            // (the minimum for conInterval)
            if self.interval < 1_250 {
                return Err(StateError::InvalidConfig("Interval for discovering AAs was shorter than the minimum connection interval (1.25 ms)."));
            }

            // Everything was ok and is set
            Ok(())
        }
        else {
            Err(StateError::MissingConfig(
                "Config necessary for discovering AAs."
            ))
        }
    }

    /// Start listening on first channel.
    /// Empty AA cache
    fn initialise(
        &mut self, parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Option<StateReturn> {
        // Fresh cache
        self.aa_cache = Queue::u8();
        // start listening on channel 0
        self.current_channel = 0;

        // Get the current channel from the channel chain
        let channel : u8 = self.channel_chain[self.current_channel];

        // Config the radio
        parameters.radio.prepare_for_config_change();
        if let Err(e) = parameters.radio.config_discover_access_addresses(self.phy , channel) {
            rprintln!("Error init discovering AAs: {:?}",e);
            panic!()
            // TODO return gracefully so device can start to idle?
            //return Err(StateError::JamBLErHalError(""));
        }

        // Set us up to receive an interval timer interrupt every self.interval microseconds
        let mut ret = StateReturn::new();
        ret.timing_requirements = Some(IntervalTimerRequirements::Periodic(self.interval));
        Some(ret)
    }

    /// Starts receiving
    #[inline]
    fn launch(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) {
        rprintln!("Launched sniffing for AAs.");

        // launch the radio
        parameters.radio.receive();
    }

    /// Updates the given parameters of the radio without changing the cache
    /// and only resetting the interval timer if the interval changed.
    /// It is not very efficient but as long as its not a problem I don't care.
    fn update_state(
        &mut self, parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Result<Option<StateReturn>, StateError> {

        // remember the current channel index
        let cur_chan = self.current_channel;
        // Remember what got changed. Default is it got changed but it will be checked.
        let mut interval_change = true;
        let mut channel_chain_change = true;
        let mut phy_change = true;

        // change the state struct to a valid one if the required parameters are missing by keeping them the same

        match parameters.config {
            Some(ref mut c) => {
                // Check for interval
                match c.interval {
                    None => {
                        // No new interval given, give it current one
                        c.interval = Some(self.interval);
                        interval_change = false;
                    }
                    Some(_) => {}
                };

                // Check for phy change
                match c.phy {
                    None => {
                        // No new phy given, give it current one
                        c.phy = Some(self.phy);
                        phy_change = false;
                    }
                    Some(_) => {}
                };

                // Check for channel_chain change
                match c.channel_chain {
                    None => {
                        // No new channel_chain given, give it current one
                        c.channel_chain = Some(self.channel_chain.clone());
                        channel_chain_change = false;
                    }
                    Some(_) => {}
                };
                

            },
            None => {
                return Err(StateError::InvalidConfig("No config provided for discovering AAs update"));
            }
        }
        
        

        // set the new configuration to this local struct, validating them as well
        self.config(parameters)?;

        if !channel_chain_change {
            // if the channel chain did not change, reset the current channel index
            self.current_channel = cur_chan;
        }

        // if the channels or the phy changed, alter the radio
        if channel_chain_change || phy_change {
            // Get the channel to change to
            let channel : u8 = self.channel_chain[self.current_channel];



            // Configure the radio
            parameters.radio.prepare_for_config_change();
            if let Err(e) = parameters.radio.config_discover_access_addresses(self.phy , channel) {
                rprintln!("Error init discovering AAs: {:?}",e);
                panic!()
                // TODO return gracefully so device can start to idle?
                //return Err(StateError::JamBLErHalError(""));
            }

            // Launch the radio
            self.launch(parameters);
        }
        

        // If the interval changed, return a timingrequest for it
        // Otherwise return nothing
        if interval_change {
            let mut ret = StateReturn::new();
            ret.timing_requirements = Some(IntervalTimerRequirements::Periodic(self.interval));
            Ok(Some(ret))
        }
        else {
            Ok(None)
        }
    }

    fn stop(&mut self, parameters: &mut StateParameters<impl JamBLErHal>) {
        rprintln!("Stopped sniffing for AAs.");
        // reset is done while changing states, I don't really have to do anything
    }

    /// Handle a radio interrupt
    #[inline]
    fn handle_radio_interrupt(
        &mut self,
        parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Option<StateReturn> {

        // TODO let the function return whether or not it was found by the master
        if let Some((aa, rssi)) = parameters.radio.read_discovered_access_address() {
            TimeStamp::rprint_normal_with_micros_from_microseconds(parameters.current_time);
            rprintln!("Found access address {:#010x} with rssi {}", aa, rssi);

            // Build discovered access address


            let mut ret = StateReturn::new();
            ret.state_message = Some(StateMessage::AccessAddress(DiscoveredAccessAddress {
                address : aa,
                phy: self.phy,
                channel:self.channel_chain[self.current_channel],
                time: parameters.current_time,
                rssi: rssi,
                sent_by_master: None,
            }));
            Some(ret)
        }
        else {
            // Delete later
            //TimeStamp::rprint_normal_with_micros_from_microseconds(instant_in_microseconds);
            //rprintln!("Discovering aas radio interrupt but not valid packet.");
            None
        }
    }

    /// Will get called every 3 seconds
    #[inline]
    fn handle_interval_timer_interrupt(
        &mut self,
        parameters: &mut StateParameters<impl JamBLErHal>
    ) -> Option<StateReturn> {

        // Change channel
        // Could do modulo, but I think it is very slow so I do it this way
        self.current_channel += 1;
        // Wrap around chain when necessary
        if !(self.current_channel < self.channel_chain.len()) {
            self.current_channel = 0;
        }

        let channel = self.channel_chain[self.current_channel];


        parameters.radio.prepare_for_config_change();
        if let Err(e) = parameters.radio.config_discover_access_addresses(self.phy , channel) {
            rprintln!("Error changing channel while discovering AAs: {:?}",e);
            panic!()
            // TODO return gracefully so device can start to idle?
            //return Err(StateError::JamBLErHalError(""));
        }
        parameters.radio.receive();

        // TODO delete this later
        TimeStamp::rprint_normal_with_micros_from_microseconds(parameters.current_time);
        rprintln!(" listening for AAs on channel {}", channel);

        None
    }

    /// Should only go to the idle state.
    fn is_valid_transition_to(&mut self, new_state: &JamBLErState) -> Result<(), StateError> {
        match new_state {
            JamBLErState::Idle => Ok(()),
            _ => Err(StateError::InvalidStateTransition(
                "Can only transition to Idle from discover AAs.",
            )),
        }
    }

    /// Should only transition to this from the idle state.
    fn is_valid_transition_from(&mut self, old_state: &JamBLErState) -> Result<(), StateError> {
        match old_state {
            Idle => Ok(()),
            _ => Err(StateError::InvalidStateTransition(
                "Can only start discovering AAs starting from the Idle state.",
            )),
        }
    }
}
