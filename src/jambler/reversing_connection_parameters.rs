/// This file contains all necessary algorithms to derive the necessary connection parameters out of captured packets.
/// 
/// Important notes:
///     - For harvested packets of which you did not know whether their crc was ok because you were not sure of the crc init: check if their reversed_crc_init is the same as the crc init you settled on. If it is the same that means the packet was correctly received! This way you can check the correctness afterwards without saving the whole packet!