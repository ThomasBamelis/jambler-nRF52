/// This file contains all necessary algorithms to derive the necessary connection parameters out of captured packets.
/// 
/// Important notes:
///     - For harvested packets of which you did not know whether their crc was ok because you were not sure of the crc init: check if their reversed_crc_init is the same as the crc init you settled on. If it is the same that means the packet was correctly received! This way you can check the correctness afterwards without saving the whole packet!
/// 
/// 
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