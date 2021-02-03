use crate::jambler::state::IntervalTimerRequirements;
use crate::jambler::state::StateConfig;
use heapless::{
    consts::*,
    spsc::{Queue},
};

use super::{JammerState, HandlerReturn};
use super::super::{JamBLErHal, BlePHY};


use rtt_target::rprintln; 

/// Struct used to hold state for sniffing access adresses on data channels.
pub struct DiscoverAas {
    /// Indicates which of the 37 datachannels have already been listened to.
    channel_map : [bool; 37],
    /// Cache for adresses already seen.
    /// A queue holding the access address as an unsigned 32 bit.
    aa_cache : Queue<u32, U255, u8>,
    /// The PHY the sniffer is listening to
    phy : BlePHY,
    /// The channel currently listening on
    channel : u8,
    /// Time when started to listen on this channel in milliseconds
    start_time_this_channel : u64,
}


impl JammerState for DiscoverAas {

    fn new() -> DiscoverAas {
        DiscoverAas {
            channel_map : [false; 37],
            aa_cache : Queue::u8(),
            phy : BlePHY::Uncoded1M,
            channel : 0,
            start_time_this_channel : 0
        }
    }

    fn config(&mut self, parameters : StateConfig) -> bool {
        if let Some(phy) = parameters.phy {
            self.phy = phy;

            true
        }
        else {
            false
        }
    }

    /// Start listening on channel 1.
    /// 
    fn initialise(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64) -> IntervalTimerRequirements {
        self.channel_map = [false; 37];
        self.aa_cache = Queue::u8();
        // start listening on channel 1
        self.channel = 1;
        // change channel every 3 seconds
        IntervalTimerRequirements::Periodic(3 * (1_000_000))
    }

    fn launch(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64) {
        
        rprintln!("Launched sniffing for AAs.");
    }

    

    fn stop(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64) {
        rprintln!("Stopped sniffing for AAs.");
    }

    /// Handle a radio interrupt
    fn handle_interrupt(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64)  -> (HandlerReturn, IntervalTimerRequirements)   {


        rprintln!("Found new access address {}.", 1);
        (HandlerReturn::NoReturn, IntervalTimerRequirements::NoChanges)
    }

    /// Will get called every 3 seconds
    fn handle_interval_timer_interrupt(&mut self, radio : &mut impl JamBLErHal, instant_in_microseconds : u64)  -> (HandlerReturn, IntervalTimerRequirements) {

        self.channel = (self.channel + 1) % 10;

        rprintln!("Sniffing AAs on new channel {}. Timestamp: d {} h {} min {} s {} ms {}", self.channel, (instant_in_microseconds / (24 * 60 * 60 * 1000_000)), (instant_in_microseconds / (60 * 60 * 1000_000)) % 24, (instant_in_microseconds / (60 * 1000_000)) % 60, (instant_in_microseconds / 1_000_000) % 60 , (instant_in_microseconds / 1000) % 1000);
        //rprintln!("Sniffing AAs on new channel {}. Timestamp: {}", self.channel, instant_in_microseconds);
        
        (HandlerReturn::NoReturn, IntervalTimerRequirements::NoChanges)
        
    }
}

