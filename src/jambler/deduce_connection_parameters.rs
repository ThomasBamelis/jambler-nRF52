use gcd::Gcd;
//use rtt_target::rprintln;
//use crate::ConnectionSample;
use super::ConnectionSample;
use crate::jambler::BlePhy;

//use heapless::HistoryBuffer;
use heapless::{consts::*, spsc::Queue, BinaryHeap, binary_heap::Max};


// See thesis text
const CONN_INTERVAL_THRESSHOLD: u8 = 10;
const CRC_INIT_THRESSHOLD: u8 = 5;


type ConnectionSampleQueue = Queue<ConnectionSample, U32>;
type UnusedChannelQueue = Queue<u8, U32>;
type RecentCrcInitSamples = Queue<u32, U10>;
type AnchorPoints = Queue<AnchorPoint, U256>;
/// (CI, Option<(conn_interval, channel_map, absolute_time_start, drift_from_start, crc_init)>)
type FoundParameters = (CounterInterval, Option<(u32, u64, u64, i64, u32)>);

#[derive(Clone, Copy, PartialEq)]
enum ChannelMapEntry {
    Unknown,
    /// unused will be overwritten by used no matter what
    Unused,
    Used,
}

/// We want to end up with 1 interval with exactly 1 solution and all the rest NoSolutions.
/// NoSolutions means basically "finished", as does exactly one.
/// All NoSolutions means there is a contradiction.
#[derive(Clone, Copy, PartialEq)]
pub enum CounterInterval {
    Unknown,
    /// Acceptable end state if it is the only one.
    /// Keep the version to know we need more info if we find multiple ones
    /// (counter, version)
    ExactlyOneSolution(u16, u16),
    /// Indicates there were mutliple solutions and we need more information
    /// Holds the version this was found in (to deferentiate we need more info or we just didn't run it again for the new info)
    MultipleSolutions(u16),
    /// Always acceptable end state
    NoSolutions,
}

#[derive(Debug)]
struct AnchorPoint {
    /// The absolute time the anchorpoint was caught as a multiple of 1250. 2**16*1250 longer than 4_000_000
    pub channel: u8,
    pub time_diff_with_prev: u64,
}


/// A wrapper for all necessary control information for the task used for deducing connection parameters.
/// This is the message passing struct between the host and the task.
pub struct DeduceConnectionParametersControl {
    pub access_address: u32,
    pub master_phy: BlePhy,
    pub slave_phy: BlePhy,
    pub connection_sample_queue: ConnectionSampleQueue, // TODO split into producer and consumer
    pub unused_channel_queue: UnusedChannelQueue,
    pub reset: bool,
}

impl DeduceConnectionParametersControl {
    pub fn new() -> DeduceConnectionParametersControl {
        DeduceConnectionParametersControl {
            access_address: 0,
            connection_sample_queue: Queue::new(),
            unused_channel_queue: Queue::new(),
            reset: false,
            master_phy: BlePhy::Uncoded1M,
            slave_phy: BlePhy::Uncoded1M,
        }
    }

    /// Resets the control block and returns the access address it holds.
    pub fn reset(&mut self) -> (u32, BlePhy, BlePhy) {
        self.connection_sample_queue = Queue::new();
        self.unused_channel_queue = Queue::new();
        self.reset = false;
        (self.access_address, self.master_phy, self.slave_phy)
    }
}

impl Default for DeduceConnectionParametersControl {
    fn default() -> Self {
        Self::new()
    }
}

/// 
/// ## Everything is public for testing purposes
pub struct DeductionState {
    channel_map: [ChannelMapEntry; 37],
    crc_init: u32,
    // the maximum observed connection interval in microseconds
    // defaults to 4 seconds, which is the maximum according to the BLE specification
    smallest_time_delta: u32,
    absolute_time_reference_point: u64,
    time_prev_anchor_point : u64,
    access_address: u32,
    master_phy: BlePhy,
    slave_phy: BlePhy,
    recent_crc_init_samples: RecentCrcInitSamples,
    anchor_points: AnchorPoints,
    /// Indicates wether we started processing already because we thought we had the correct connInterval, crc init and channel map
    processing: bool,
    total_packets: u32,
    new_packets: u32,
    new_anchor_points: u32,
}

impl DeductionState {
    /// Used for initialising the static variable
    pub const fn new() -> DeductionState {
        DeductionState {
            channel_map: [ChannelMapEntry::Unknown; 37],
            crc_init: core::u32::MAX,
            smallest_time_delta: 4_000_000,
            absolute_time_reference_point: core::u64::MAX,
            time_prev_anchor_point: 0,
            access_address: 0,
            master_phy: BlePhy::Uncoded1M,
            slave_phy: BlePhy::Uncoded1M,
            recent_crc_init_samples: Queue(heapless::i::Queue::new()), // Should be HISTORY BUFFER
            /// TODO Should be HISTORY BUFFER OR MIN BINARYHEAP SORTED ON TIME SO IT WILL ALWAYS BE ORDENED FOR MULTIPLE DEVICES
            anchor_points: Queue(heapless::i::Queue::new()), // Should be HISTORY BUFFER OR MIN BINARYHEAP SORTED ON TIME SO IT WILL ALWAYS BE ORDENED FOR MULTIPLE DEVICES
            processing: false,
            total_packets: 0,
            new_packets: 0,
            new_anchor_points: 0,
        }
    }

    pub fn reset(&mut self, new_access_address: u32, master_phy: BlePhy, slave_phy: BlePhy) {
        self.channel_map = [ChannelMapEntry::Unknown; 37];
        self.crc_init = core::u32::MAX;
        // the maximum observed connection interval in microseconds
        // defaults to 4 seconds, which is the maximum according to the BLE specification
        self.smallest_time_delta = 4_000_000;
        // The time of the first caught packet
        self.absolute_time_reference_point = core::u64::MAX;
        self.time_prev_anchor_point= 0;
        self.access_address = new_access_address;
        self.master_phy = master_phy;
        self.slave_phy = slave_phy;
        self.recent_crc_init_samples = Queue::new();
        self.anchor_points = Queue::new();
        self.processing = false;
        self.total_packets = 0;
        self.new_packets = 0;
        self.new_anchor_points = 0;
    }

    pub fn get_nb_packets(&self) -> u32 {
        self.total_packets
    }

    pub fn get_access_address(&self) -> u32 {
        self.access_address
    }

    pub fn get_master_phy(&self) -> BlePhy {
        self.master_phy
    }

    pub fn get_slave_phy(&self) -> BlePhy {
        self.slave_phy
    }

    /// Will process all elements in both queues and update the version.
    /// Returns the smallest delta seen and a possibly new crc init.
    /// For use with the simple algorithm.
    pub fn process_new_information_simple(
        &mut self,
        connection_sample_queue: &mut ConnectionSampleQueue,
        unused_channel_queue: &mut UnusedChannelQueue,
    ) -> (Option<u32>, Option<u32>) {
        if !connection_sample_queue.is_empty() || !unused_channel_queue.is_empty() {

            self.new_anchor_points = 0;
            self.new_packets = 0;
            let mut smallest_time_delta = None;

            // there is new info
            // Process any new connection samples
            while let Some(connection_sample) = connection_sample_queue.dequeue() {
                if let Some(time_delta) = self.process_connection_sample_simple(connection_sample){
                    let (time_delta, _) = DeductionState::round_to_1250_with_abs_diff(time_delta);

                    // 1250 is to skip first 0, and only remember if smaller than smallest ever seen
                    if time_delta > 1250 && time_delta < self.smallest_time_delta {
                        self.smallest_time_delta = time_delta;
                        smallest_time_delta = Some(time_delta);
                    }
                }
            }

            // Process any new unused channels
            while let Some(unused_channel) = unused_channel_queue.dequeue() {
                // Used has precedence over unused, only change to unused if unknown
                if let ChannelMapEntry::Unknown = self.channel_map[unused_channel as usize] {
                    self.channel_map[unused_channel as usize] = ChannelMapEntry::Unused;
                }
            }

            // check crc
            let mut nb_occured: u8 = 0;
            let mut option_new_crc_init = None;
            for crc_init in self.recent_crc_init_samples.iter() {
                // Count how many times this one occurs in the recently seen crc inits
                for other_crc_inits in self.recent_crc_init_samples.iter() {
                    if crc_init == other_crc_inits {
                        nb_occured += 1;
                    }
                }

                // If above threshold and not the same as the one we already have
                if nb_occured >= CRC_INIT_THRESSHOLD && *crc_init != self.crc_init {
                    // Found one that occurs as much as we want, save it internally and signal jambler
                    self.crc_init = *crc_init;
                    option_new_crc_init = Some(*crc_init);
                    break
                } 

                nb_occured = 0;
            }

            // check if we have everything
            /*    Check if we can start processing if we didn't yet     */
            if !self.processing {
                self.processing = true;
                // check if we already have the channel map
                for channel in self.channel_map.iter() {
                    if let ChannelMapEntry::Unknown = channel {
                        self.processing = false;
                        break;
                    }
                }
                // Check if we are above the anchor point threshold to believe the conn interval
                if self.anchor_points.len() as u32 <= (CONN_INTERVAL_THRESSHOLD + 1) as u32 {
                    self.processing = false;
                }
                // Check if we have a crc init (Not strictly necessary tho)
                if self.crc_init == core::u32::MAX {
                    self.processing = false;
                }
                
            }

            return (smallest_time_delta, option_new_crc_init);
        }
        (None, None)
    }

    /// Processes a connection sample.
    /// For use with the simple algorithm, using smallest anchor point time delta for feedback and smallest n to determine conn_interval.
    fn process_connection_sample_simple(&mut self, connection_sample: ConnectionSample) -> Option<u32> {
        // adapt channel map, restart processing if this changes the channel map and we are already processing
        self.channel_map[connection_sample.channel as usize] = ChannelMapEntry::Used;

        // update counters
        self.total_packets += 1;
        self.new_packets += 1;

        // Enqueue, pop if necessary
        if let Err(crc_init) = self
        .recent_crc_init_samples.enqueue(connection_sample.packet.reversed_crc_init) {self
            .recent_crc_init_samples.dequeue();self
            .recent_crc_init_samples.enqueue(crc_init).unwrap();};
        // And the one from the response if there is one
        if let Some(response) = connection_sample.response.as_ref() {
            if let Err(crc_init) = self.recent_crc_init_samples.enqueue(response.reversed_crc_init) {self
                .recent_crc_init_samples.dequeue();self
                .recent_crc_init_samples.enqueue(crc_init).unwrap();};
        }

        // Add to anchor points if it is an anchor point
        // And do counter and relative processing in the meantime
        if self.is_anchor_point(&connection_sample) {
            // update counters
            self.new_anchor_points += 1;

            // If it is the first anchor point, set it as the reference point
            if self.anchor_points.is_empty() {
                // You can get total difference by absolute - point
                self.absolute_time_reference_point = connection_sample.time;
            } 

            let time_to_prev_anchor_point = connection_sample.time - self.time_prev_anchor_point;

            // Add as anchor point
            let new_anchor_point: AnchorPoint = AnchorPoint {
                channel: connection_sample.channel,
                time_diff_with_prev : if self.time_prev_anchor_point == 0 {
                    0
                }
                else {
                    time_to_prev_anchor_point
                }
            };

            // Remember time
            self.time_prev_anchor_point = connection_sample.time;

            // Enqueue (pop previous anchor point if needed)
            if let Err(new_anchor_point) = self.anchor_points.enqueue(new_anchor_point) {self.anchor_points.dequeue();self.anchor_points.enqueue(new_anchor_point).unwrap();};

            Some(time_to_prev_anchor_point as u32)
        }
        else {
            None
        }
    }


    /// Pattern matches the assigned interval and returns its findings.
    /// Returns (CI, Option<(conn_interval, channel_map, absolute_time_start, drift_from_start)>).
    /// This is the simple version of brute forcing the initial counter.
    /// Depending on the actual impact of the slowness, an untested buggy distributed processing version has been made but is not in use right now.
    /// The channel map array, drift and conn_interval are only computed here.
    pub fn process_interval_simple(
        &mut self
    ) -> FoundParameters {
        if !self.processing {
            return (CounterInterval::Unknown, None);
        }
        
        // calculate everything we need for simplicity!
        // We only need self.aa, self.processing, self.anchor_points and the channelmap entries

        // cast to u32s to circumvent a whole bunch of other casting
        let channel_identifier = calculate_channel_identifier(self.access_address) as u32;
        let channel_map_in_u64 = DeductionState::channel_map_entries_to_mask(&self.channel_map);
        let (channel_map_bool_array, remapping_table, _, nb_used) = generate_channel_map_arrays(channel_map_in_u64);
        
        // 5 smallest in a max heap
        type NbAnchorsToConsider = U5;
        let mut n_smallest_time_deltas : BinaryHeap<(u32,u32), NbAnchorsToConsider, Max> = BinaryHeap::new();
        self.anchor_points.iter().for_each(|ap|{
            // skip 0 from first
            if ap.time_diff_with_prev < 7000 {return}

            // fill until full
            let (current_time_diff, drift) = DeductionState::round_to_1250_with_abs_diff(ap.time_diff_with_prev as u32);
            if let Err((current_time_diff, drift)) = n_smallest_time_deltas.push((current_time_diff, drift)) {
                // add if smaller than largest or less drift
                let (cur_max, cur_max_drift) = n_smallest_time_deltas.peek().unwrap();
                // If they are not 3250 apart, there is a big chance they have the same number of connection events in-between, take the one with smallest error to 1250
                if current_time_diff < *cur_max - 3750 || (current_time_diff < *cur_max + 3750 && drift < *cur_max_drift) {
                    n_smallest_time_deltas.pop().unwrap();
                    n_smallest_time_deltas.push((current_time_diff, drift)).unwrap();
                }
            }
        });

        // They come in random order!
        let fold_base = n_smallest_time_deltas.peek().unwrap().0;
        let conn_interval = n_smallest_time_deltas.into_iter().fold(fold_base, |running_gcd, next_time_delta| running_gcd.gcd(next_time_delta.0));

        // Calculate drift from absolute time (first anchor point)
        let drift = self.anchor_points.iter().skip(1).map(|anchor_point| anchor_point.time_diff_with_prev as i64 - (DeductionState::round_to_conn_interval(anchor_point.time_diff_with_prev, conn_interval).0 as i64) ).sum::<i64>();

        let mut running_event_counter;

        let mut found_counter: Option<u32> = None;
        let mut inconsistency: bool;
        for potential_counter in 0..=core::u16::MAX {
            // reset inconsistency
            inconsistency = false;
            running_event_counter = potential_counter;
            for anchor_point in self.anchor_points.iter() {
                running_event_counter = (running_event_counter as u32 + (DeductionState::round_to_conn_interval(anchor_point.time_diff_with_prev, conn_interval).1) as u32) as u16;
                let channel_potential_counter = csa2_no_subevent(
                    running_event_counter as u32,
                    channel_identifier,
                    &channel_map_bool_array,
                    &remapping_table,
                    nb_used,
                );

                // If we get another one than expected, go to next counter
                if channel_potential_counter != anchor_point.channel {
                    inconsistency = true;
                    break;
                }
            }

            // If there was no inconsistency for this potential counter save it
            if !inconsistency {
                match found_counter {
                    None => {
                        // the first one without inconsistency, save it
                        found_counter = Some(potential_counter as u32);
                    }
                    Some(_) => {
                        // There was already another one without inconstistency, we have multiple solutions
                        return (CounterInterval::MultipleSolutions(0), None);
                    }
                }
            }
        }

        // The fact we get here, we did not find mutliple solutions, must be one or none.
        // Remember for exactly one you need to run through the whole range
        match found_counter {
            None => {
                // There were no solutions
                (CounterInterval::NoSolutions, None)
            }
            Some(counter) => (CounterInterval::ExactlyOneSolution(counter as u16, 0), Some((conn_interval, channel_map_in_u64, self.absolute_time_reference_point, drift, self.crc_init))),
        }
    }


    /// Something is an anchorpoint
    ///  when you were listening on the channel for longer than the worst case.
    /// The packet phy will always be the master phy and that is the one  
    fn is_anchor_point(&self, connection_sample: &ConnectionSample) -> bool {
        // declare the constants for each the on air time in micros of each phy
        // TODO account for state change time...
        static UNCODED_1M_SEND_TIME: u32 = 2128;
        static UNCODED_2M_SEND_TIME: u32 = 2128 / 2;
        static CODED_S2_SEND_TIME: u32 = 4542; // AA, CI, TERM1 in S8
        static CODED_S8_SEND_TIME: u32 = 17040;
        // TODO get from connection sample or also from reset
        let actual_slave_phy: BlePhy = self.slave_phy;
        let actual_master_phy: BlePhy = self.master_phy;

        // THE RADIO ONLY LISTENS ON MASTER PHY FIRST
        // Does not matter if you caught response or not
        // TODO changed: WRONG, the anchor point time you would take from the start of the slave would give you a wrong anchor point time.
        //let mut previous_packet_start: u32 = if actual_master_phy == actual_slave_phy {
        let mut previous_packet_start: u32 = if false {
            // If they are the same, return either one
            // You would have caught the previous packet either way because same phy
            match actual_master_phy {
                BlePhy::Uncoded1M => UNCODED_1M_SEND_TIME,
                BlePhy::Uncoded2M => UNCODED_2M_SEND_TIME,
                BlePhy::CodedS2 => CODED_S2_SEND_TIME,
                BlePhy::CodedS8 => CODED_S8_SEND_TIME,
            }
        } else {
            // If they are different, jambler would not have caught the previous one because it was listening on the wrong phy and we actually have to go back the full subevent
            let m_time = match actual_master_phy {
                BlePhy::Uncoded1M => UNCODED_1M_SEND_TIME,
                BlePhy::Uncoded2M => UNCODED_2M_SEND_TIME,
                BlePhy::CodedS2 => CODED_S2_SEND_TIME,
                BlePhy::CodedS8 => CODED_S8_SEND_TIME,
            };
            let s_time = match actual_slave_phy {
                BlePhy::Uncoded1M => UNCODED_1M_SEND_TIME,
                BlePhy::Uncoded2M => UNCODED_2M_SEND_TIME,
                BlePhy::CodedS2 => CODED_S2_SEND_TIME,
                BlePhy::CodedS8 => CODED_S8_SEND_TIME,
            };
            m_time + 150 + s_time
        };

        // account for clock drift. 50 ppm active clock drift + own clock drift
        // TODO would need own clock drift here. I know its less than 20 ppm tho for dongle.
        // Yes this incorporates range delay
        let extra_delay_percentage: f32 = 1.0 + (50.0 + 20.0) / 1_000_000.0;
        previous_packet_start =
            ((previous_packet_start as f32) * extra_delay_percentage) as u32 + 1;

        // account for active clock drift master and my clock drift and allowance
        // 2 ms allowance + range delay for 3 km
        previous_packet_start += 2 + 24;

        // If we listened for longer than the time it would have taken to reach this, return true
        previous_packet_start < connection_sample.time_on_channel
    }


    fn round_to_1250_with_abs_diff(number : u32) -> (u32, u32) {
        let mod_1_25_ms : u32 = number % 1250 ;
        // get it to the closest counter point from reference
        let discrete_relative_timepoint : u16;
        if mod_1_25_ms < (1250 / 2) {
            // closest to lower counter point, just let / drop it
            discrete_relative_timepoint = (number / 1250) as u16;
        } else {
            // closest to upper value counter point, round to upper by + 1
            discrete_relative_timepoint = (number / 1250) as u16 + 1;
        }
        let rounded : u32 = 1250 * discrete_relative_timepoint as u32;
        let diff = (number as i32 - rounded as i32).abs() as u32;
        (rounded, diff)
    }

    fn round_to_conn_interval(number : u64, conn_interval : u32) -> (u32, u16) {
        let mod_conn_int : u32 = (number % conn_interval as u64) as u32 ;
        // get it to the closest counter point from reference
        let discrete_relative_timepoint : u16;
        if mod_conn_int < (conn_interval / 2) {
            // closest to lower counter point, just let / drop it
            discrete_relative_timepoint = (number / conn_interval as u64) as u16;
        } else {
            // closest to upper value counter point, round to upper by + 1
            discrete_relative_timepoint = (number / conn_interval as u64) as u16 + 1;
        }
        let rounded : u32 = conn_interval * discrete_relative_timepoint as u32;
        (rounded, discrete_relative_timepoint)
    }


    /// Turns the channel map entries into a u64 bit mask
    fn channel_map_entries_to_mask(entries: &[ChannelMapEntry; 37]) -> u64 {
        let mut channel_map_in_u64: u64 = 0;
        (0..entries.len()).for_each(|channel| {
            if let ChannelMapEntry::Used = entries[channel] {
                channel_map_in_u64 |= 1 << channel;
            } else if entries[channel] != ChannelMapEntry::Unused {
                panic!("Channel map was not complete in used/unused when trying to create a u64 mask for it")
            }
        });
        channel_map_in_u64
    }

}

/*********************************************************************************
 *
 * TODO CSA#1
 *
 ********************************************************************************/

/*********************************************************************************
 *
 * Channel Selection Algorithm #2
 *
 ********************************************************************************/

/// Calculates the channel identifier from the access address.
///
/// Only calculate on access address change.
fn calculate_channel_identifier(access_address: u32) -> u16 {
    (((access_address >> 16) as u16) ^ (access_address as u16)) as u16
}

/// Generates a bunch useful arrays out of a channel map delivered as a u64 bit mask.
/// Returns (channel_map_array, remapping_table, inverse_remapping_table, nb_used).
///
/// Only calculate on channel map change.
#[inline(always)]
fn generate_channel_map_arrays(channel_map: u64) -> ([bool; 37], [u8; 37], [u8; 37], u8) {
    let mut nb_used: u8 = 0;
    let mut channel_map_array = [false; 37];
    let mut remapping_table = [0xFF; 37];
    let mut inverse_remapping_table = [0xFF; 37]; // for subevents, when you need remapping index
    for channel_index in 0u8..37u8 {
        if channel_map & (1 << channel_index) != 0 {
            //
            channel_map_array[channel_index as usize] = true;
            // add to remapping table (in ascending order as specs say)
            remapping_table[nb_used as usize] = channel_index;
            // get this to to have O(1) remapping index
            inverse_remapping_table[channel_index as usize] = nb_used;
            // remember how many channels
            nb_used += 1;
        }
    }
    (
        channel_map_array,
        remapping_table,
        inverse_remapping_table,
        nb_used,
    )
}

/// Calculate the channel for the given counter, channel identifier and channel map.
/// Uses u32 internally because of overflow it will run into u32 multiple times and instead of casting thousands of time, just reuse the u32s.
fn csa2_no_subevent(
    counter: u32,
    channel_identifier: u32,
    channel_map: &[bool; 37],
    remapping_table: &[u8; 37],
    nb_used: u8,
) -> u8 {
    // calculate "pseudo random number e", figure 4.46
    let mut prn_e: u32;
    prn_e = counter ^ channel_identifier; // xor
    prn_e = perm(prn_e); // perm
    prn_e = mam(prn_e, channel_identifier); // mam
    prn_e = perm(prn_e); // perm
    prn_e = mam(prn_e, channel_identifier); // mam
    prn_e = perm(prn_e); // perm
    prn_e = mam(prn_e, channel_identifier); // mam
    prn_e ^= channel_identifier;

    // figure 4.47
    let unmapped_channel: u8 = (prn_e % 37) as u8;

    // figure 4.48
    if channel_map[unmapped_channel as usize] {
        // used channel
        unmapped_channel
    } else {
        // remap
        let remapping_index = (((nb_used as u32) * (prn_e as u32)) >> 16) as usize;
        remapping_table[remapping_index]
    }
}

/// Operation block in the CSA#2 algorithm.
/// Switches the byte by first switching bits next to each other, pairs next to each other, then 4bits next to each other.
/// This results in each separate byte switched.
#[inline(always)]
fn perm(mut input: u32) -> u32 {
    input = ((input & 0xaaaa) >> 1) | ((input & 0x5555) << 1);
    input = ((input & 0xcccc) >> 2) | ((input & 0x3333) << 2);
    input = ((input & 0xf0f0) >> 4) | ((input & 0x0f0f) << 4);
    input
}

/// Operation block in the CSA#2 algorithm.
#[inline(always)]
fn mam(a: u32, b: u32) -> u32 {
    let mut ret: u32;
    //ret = a as u32 * 17; // cannot overflow! upgrade to u32
    // a * 17 = a * 2^4 + a
    ret = (a << 4) + a;
    ret += b;
    // mod 2^16
    ret & 0xFFFF
}

/*********************************************************************************
 *
 * REVERSE CALCULATING THE CRC INIT
 *
 ********************************************************************************/

/// This file contains all necessary algorithms to derive the necessary connection parameters out of captured packets.
///
/// Important notes:
///     - For harvested packets of which you did not know whether their crc was ok because you were not sure of the crc init: check if their reversed_crc_init is the same as the crc init you settled on. If it is the same that means the packet was correctly received! This way you can check the correctness afterwards without saving the whole packet!
///
///
/// 
/// TODO use the crc_any crate, most likely much faster
pub fn reverse_calculate_crc_init(received_crc_value: u32, pdu: &[u8], pdu_length: u16) -> u32 {
    let mut state: u32 = reverse_bits_u32(received_crc_value) >> 8;
    let lfsr_mask: u32 = 0xb4c000;

    // loop over the pdu bits (as sent over the air) in reverse
    // The first processed bit is the 0b1xxx_xxxx bit of the byte at index pdu_length of the given pdu
    for byte_number in (0..pdu_length).rev() {
        let current_byte: u8 = pdu[byte_number as usize];
        for bit_position in (0..8).rev() {
            // Pop position 0 = x^24
            let old_position_0: u8 = (state >> 23) as u8;
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
    let mut ret: u32 = 0;
    // Go from CRC_init most significant to least = pos23->pos0
    for i in 0..24 {
        ret |= ((state >> i) & 1) << (23 - i);
    }

    ret
}


/// TODO use the trick below, but adapt. now it contains each byte reversed, so now reverse the bytes using 0xff00ff00 >> 8, 0x00ff00ff << 8 and 0xffff0000 >> 16, 0x0000ffff << 16
/// TODO NOPE JUST USE CRC_ANY CRATE, THESE FUNCTION WILL NOT BE NECESSARY ANYMORE THEN
/// ```
/// input = ((input & 0xaaaa) >> 1) | ((input & 0x5555) << 1);
/// input = ((input & 0xcccc) >> 2) | ((input & 0x3333) << 2);
/// input = ((input & 0xf0f0) >> 4) | ((input & 0x0f0f) << 4);
/// ```
fn reverse_bits_u32(byte: u32) -> u32 {
    let mut reversed_byte: u32 = 0;
    // Go right to left over original byte, building and shifting the reversed one in the process
    for bit_index in 0..32 {
        // Move to left to make room for new bit on the right (new LSB)
        reversed_byte <<= 1;
        // If byte is 1 in its indexed place, set 1 to right/LSB reversed
        if byte & (1 << bit_index) != 0 {
            reversed_byte |= 0b0000_0001;
        } else {
            reversed_byte |= 0b0000_0000;
        }
        //reversed_byte |= if byte & (1 << bit_index) != 0 {0b0000_0001} else {0b0000_0000};
    }
    reversed_byte
}

