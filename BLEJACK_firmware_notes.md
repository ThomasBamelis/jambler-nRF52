To whoever read this: I did not plan on sharing this so forgive my language, if this ever goes public, all traces of this file need to be deleted.
This markdown contains my notes on the [BLEJACK firmware code](https://github.com/virtualabs/btlejack-firmware/tree/2125e095d05f446fc2d7d050ca7e14b1db137759) in the repository.
Basically the whole firmware is in the main file, which contains one interrupt handler for radio requests, that gets called when the radio has one of its configured events that fires. 
I will explain the files first:
<dl>
  <dt>helpers</dt>
  <dd>As you can guess these are helper functions. It has some helper function for reversing a cyclic redundancy check (?), dewhitening in ble (?), access address validity check and channel map (CHM in his code) to array and vice versa. The code is a bit random but not too complicated.</dd>
  <dt>link</dt>
  <dd> This files is for the serial connection via the microbit to the python program running on the connected computer. <br /> link.h is quite full of defines as it defines the packet layout of the things he sends back and forth with the python mother program.</dd>
  <dt>Main</dt>
  <dd>Very large were he keeps the state of his sniffer and were apparently some of his most important functions are located.
Among others the greatest common devisor method for deriving the hopcount of BLE5 and deriving the initial counter from a couple of captured packets.
This is were he does the smart stuff to hack the connections, which makes me believe the other files are helper files to make the actual bleutooth connection work and are "actual" firmware functions he uses to make the bluetooth work.
</dd>
  <dt>Radio</dt>
  <dd>Contains the primitive commands he sends to the RF module by writing to the registries of the Nordic chip.
He prepares the radio to follow and listen on certain channels, switch channels, start sending on channels,...
At one point in main.cpp he mentions the nRF51 manual when he is setting some registers, so this might be a good hint to see at what level you will be starting on. 
The reference is this: nRF51 Series Reference Manual v2.1, section 6.1.1, page 18.<br />
The only include is radio.h and the only include in that file is MicroBit.h, so my guess is that the defined constants he uses to operate the radio peripheral are part of the MicroBit library so maybe he is not that bare metal. On further looking into it, it looks like he is accessing the nrf51.h constants given by the actual chip manufacturer (same names as in my datasheet) through the microbit library because it probably puts them available as well. This makes sense cause the code he uses is the same for the adafruit sniffer as well which is the same nRF51 chip but not a microbit. So I'll have to find a way to work with the manufacturer c++ library.
</dd>
  <dt>sequence</dt>
  <dd>In this file he implements the channel hopping algorithm of BLE 5 as well as the patter matching given some packets. So given the seed and counter you can get the next channel here.</dd>
  <dt>timer</dt>
  <dd>This is a file he uses written by other people for operating the timers of the nRF51 very low level. The timer functions are pretty basic but the code is hard and unreadble because it interacts directly with the hardware registers. HE USES A C(++?) LIBRARY FROM NORDIC FOR THE CONSTANTS HERE!!!!! nrf51.h and nrf51_bitfields.h !!!! very good news, means I will have it too and I can start looking at the primitives it has. AND THE TYPE OF WRITING IS THE SAME AS IN THE RADIO MODULE WITH NRF_...!_ very good news</dd>
</dl>


# Commands
### List AAs
Sets the sniffer in SNIFF_AA mode and channel to 1. Reset pkt count.
He then configures the radio to listen to all AAs on channel 1, 
using the trick of setting the adress to listen to, to the preamble (AA=BASE|PREFIX=0..0|AA=0..0|10101010). (prefix is prefix for radio, LSBF)
Using this trick, you fool the chip into matching on the preamble of every packet and so the access address received will be in the length.
  **AA FOR CODED CHANNELS IS DIFFERENT**. NRF_RADIO->PREFIX0 = 0x8e;
  NRF_RADIO->BASE0 = 0x89bed600; Sets the radio to listen to address 0x8E89BED6.
  For LE 2M the preamble is 16 bits AAAA or 01010101... . FOr coded PHY  is 10 repetitions of 0x3C.
  The preamble is either 010101... or 10101010, the first bit has to be the same as the first bit of the access adress.
  On coded phy, the preamble is not coded, but the access address always is with s=8. So you would need to make it so that the access address you listen to is equal to 0..0AAAA when coded. This wil get fucked up very fast, because you would also need to do this inversion to read out the access addresss afterwards, which we will receive as a possible combination of s=3,s=8. ~~Try to find a way so that it matches all packets (i think you can do this...)~~ -> not possible or figure out what one of my notes said that they all broadcast their used access address so that nobody else takes it, so I would be able to easily read it from an advertising channel?.
  If hard mode: the coded PHYs send on 1M but coded -> listen for their preamble which is uncoded = 10 times 0x3c, then you should be able to receive all coded bits if length was long enough, then decode them. Use STATLEN in PCNF1 to always receive enough bytes. If maxlen=statlen=10 you will always alwyas listen for 10 bytes. Also set balen to 1. By default, payload and all are also sent over the air in LSBit, thats why he can just read the access address as it is sent like that.
  IMPORTANT: you should turn of data whitening when you sniff for AAs or it might fuck up what you receive. S0, length, S1 are part of the PDU, which get whitened.
  The access address does not get whitened. -> if you want to read the S0 en length (pdu header), you have to dewithen what you read in in bytes 4 and 5 from the read buffer manually. 

So, then when a radio event occurs (he receives a packet):
Regardless of how you configre the s0.s1 and len lengths, for ble4.
He checks the pdu header (dewhitened bytes 4 and 5) and will only add an AA if the lenght is null and before is 10xx0000 -> empty LL data pdu, for ble 5 must be 10xx0x00. (his dewhiten function reverses the bits I think, ~~or every byte is sent in reverse, because he does not reverse AA while it is sent LSB first.~~ yes, the statndard is for things to be sent over the air LSBit, thats why he can read access address and why he kicks out the rightmost = LSBit when he shifts. So the only conclusion left is that his manual dewithening reverses the bits)
This way, he filters for empyt LL Data PDUs, which the master sends to a slave in a connection so the slave is allowed to send data.
He reads the access address from bytes 0-3 in the receive buffer.
He will try this once, if it is garbage he will bitshift to the rightthe first 9 bytes (the weird thing puts the first bit of the following rigister as the left most bit of this one (why, doesnt make sense)) and try again.
What he is doing wit this is moving all bits to the right, kicking out the rightmost bit of the first byte, which for my guess is were an anwanted preamble bit would have entered.
He probably does this because he always puts as access address preamble AA, but it can als be 0x55, so the packet would be missaligned. I think that is why he does this.
He goes up one channel every 100 packets.

### Recover 
This command is used to figure out the CSA parameters.
However, this command has 4 options/modes, each for recovering/finding out a parameter of the CSA.
You start with only the access address, and need the result of the previous options for the next options. It goes start with access address -> crcInit -> channel map -> HopInterval & HopIncrement -> last option starts following the connection.
The channel map option also gets called if he thinks the channel map changed while already following a connection.
This was changed howerver, now before the connection following you have the BLE5 version channel map recovery and prng

#### Recover CRCInit
He will set the radio to listen to the given access address on the last used channel (of whatever he was doing before, aka I think channel doesnt matter, as long as it is a data channel, preferably mapped (has to be)).
Before this, he attaches a ticker, which increments the global measures variable, with a parameter of 1250. 
It looks like this ticker is an ARM MBED os Ticker, where you can give a function handle and a float specifying the number of milliseconds for which every so many milliseconds it will call the function.
So measures will be incremented every 1.25 seconds.
He then listens for packets on the channel, disregarding crc but enabling automatic dewhitening, only taking in the first 10 bytes heard (for the header).
Then when a packet is received, he does the same empty LL data pdu check as before.
Becuase of his static listening and empty packet, the crc is now in the received payload (byte indexes 2-4) (could not extract from dedicated register, because we dont have crc init).
He then reverses the crc operation on the crc value to the init value accordding to the paper in his helper function, giving him the init value.
He copy pasted this function from the paper, so I could just do that to.
If the crc is new he remembers it. He als sets the ticks (measure) to the max interval if it is larger than the max interval seen. 
I suspect he measure the longest time the connection has been paused, to figure out sleep or hopinterval?
He will do this 5 times (also max interval), if all 5 times the same crcinit has been found, he will return it to the user and use it as the actual crcinit.
Having done this, he will take the max_interval and mutliply it by 1.5. I think the interval might be the maximum amount of hops between 2 packets, as the increment function is called hop_ticks.
In commented out code, he then goes on to recover the rest (channel map -> hop_interval -> hop increment)

#### Recover channel map (case 3 for ble5) WHICH ALSO RECOVERS HOP INTERVAL AT THE SAME TIME
The received command has the range of channels to look for to listen to mapt them, along with the access address, the timeout value for when to assume the channel is not in use and the crcInit value.
He will then run through the channels from start to stop.
Before he will create a flag array with 0s for all cahnnels are not used.
He will also init the radio with the radio_follow_conn function.
This will legit init it for a connection, with 1 byte s0, 0 bit s1, and 8 len.
Legit 250 max payload len and no static len, 3 byte base addres (+1 prefix), litle endian and whitening and crc check.
It will only fire the radio interrupt on an end event.
**In general all his raidio inits only listen to END events for all initsin radio.c**
He will also do a ticker with 1000 times the timeout value and as incrmenter a big function that does the following:
If the channel is the stop channel (exclusive), he will stop the search and the sniffer state at this point will hold the channel map. If it is not he will increment the channel and reinitiliase the radio to the new channel (same AA and crcInit) with a different init function, radio_follow_aa.
This function does the same as conn, excpet that it uses weird lengths but most importantly IT DOES NOT RENABLE ITSELF AFTER.
S1 is 1 byte, but length is 6 bits and s1 2 bits, maxlen is 37 and static is 0.
I think both initialisation function dont matter for the channel map, they just wanna listen on a channel for an AA with a crc.
When a packet is found on the current channel, due to the mode of the sniffer (chmv5), he will check if its a channel update by checking if the last 2 bits of S0 are set (they should nver be set because they are reserved for future use?, BUT i tlook like the byte is reversed again, because setting the 2 leftmost bits indicates a LL control PDU) and if the first byte of the payload is 1, which is the place and opcode of a ll control pdu to update the channel map
(btw, a control pdu with opcode 1 (section 2.4.2.2) is a channel map update, however, pdu can be encrypted).
He will only print something, he doesnt actually do anything with this.
If this channel has not been found already (the channel currently listening on), it is indicated in the channel map and the recover_cchm_next_v5 funtion is called.
This function does a whole lot.
It looks like this function has been adapted for also working when you are reconnecting but already knowthe hopinterval, so you measure the number of hops it takes you, so you dont loose track.
NOPE: HE DOES THE HOPINTERVAL RECOVERING AT THE SAME TIME!!!!!!!
If you havent been counting hops, it is started in the state and a new ticker is added to count the hops every 1.25 seconds and increments a hopcounter.
If you are counting and you have found a packet already:
if you havent found any distance in packets already, just measure this one.
If you have he calculates the GCD of this measuremtn and the shortest seen difference yet. To avoid clock drift he does this for successive measurments which is stupid. You should crossreference with all packets found and in your gcd function allow for clock drift (I know its not easy, but you get the same amount of measurment for 4 packets as he would for 16). Also I dont think the clock drift would actually matter in the successive packets... if it does for 2 consecutive it shouldnt be that different for 3 or 4 right?
I am not sure tho, think more about this, the time count is always counten from t-startlistening. If by any change the possible hopointervals are multiples of each other, use the minimum to measure hopinterval quantity, which will lessen the impact of clock drift.
He then says he recovered the hop interval if 5 packets after one another use the current shortest interval. He says he found it then. If he also found the hopinterval, he will go to idle.
At the end he will manually trigge the ticker function, so the channel goes up one.
So what this means is that this will be ticket after $timout amount of seconds or if a new channel is found.

  **DO NOT TAKE THIS APPROACH, IN YOU CONCURRENCY WITH RTIC, JUST ADD PACKETS TO SNIFFED PACKETS ARRAY WHICH YOU INVESTIGATE WHILE IDLING**


### 4: BLE5 PRNG recovery
This expects the access address, crcInit, channel map AND as shown above the hopinterval to be found already.
Then it confirms the hopinterval is found.
It creates a sequence generator which in normal operation, given the access address (channel id) and counter would give the nextchannel. But he does not have the counter yet, so he just provides a 0 there which whill result in selecting a random data channel.
The state goes to PRNG_INIT and the radio get initialised with the radio_follow_conn as specified in the above section.
When a packet gets received now (on the given AA and channel) he now actually checks if the crc is correcty. This seems stup because you mis some packets if they are malformed and the only false positives would be if another connection bitflips in just the exact same aacess addres as we would use. I dont know the packet loss in ble, but if this is high this could be an improvement.
For some reason he starts doing the same look for hopinterval as he did before with the channel map.
If he wasnt counting, he starts to count and will actually wait for the next packet predicted by the sequence to pass and retrieve the hop interval.
It looks like this is for when he actually figures out the counter the be verry sure the hopinterval is right by seeing if the next one goes to the expected channel in the expected hopinterval. Im not sure though.
If he was counting:
It looks like he saves the differencein time (counten in hop intervals) between the previous packet and this one, and saves it. This will be used for pattern matching the prng later on.
If this is the first 5 times he does this, he asks the sequence generator again for a channel using the current counter, but this should be garbage because you simply don't know the actual counter, only you relative fictional one.
After 5 times, it looks like he already stops capturing and starts to pattern match on those 5 packets.
This resolvecounter function pattern matches and if a unique candidate remains, sets the absolute right counter value to the on found, from then on, the sequencer has the correct channels to predict.
From that moment on he will start actaully following the connection.
Jamming and sniffing become possible now.

I will not analyse his code further because now it is just a normal ble5 connection that you have to sniff.
