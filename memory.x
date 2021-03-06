
/* Linker script for the nRF52 - WITHOUT SOFT DEVICE */
/* TODO 
   I think changing the flash origin to 0x4000 should make all this compatible with the adafruit bootloader.
   
   From the adafruit website about the u2f bootloader for flashing using bossac like arduino does:

   For M4 boards, which have a 16kB bootloader, you must specify -offset=0x4000, for example:

   bossac -p=/dev/cu.usbmodem14301 -e -w -v -R --offset=0x4000 adafruit-circuitpython-feather_m4_express-3.0.0.bin

   This will erase the chip (-e), write the given file (-w), verify the write (-v) and Reset the board (-R). On Linux or MacOS you may need to run this command with sudo ./bossac ..., or add yourself to the dialout group first.

   Although I am very unsure because this might be to no override softdevice.
   What might also work is overriding the reset pointer to the start of dfu as it looks to be in 0x080000 and just having my app starting from 0.
 */

  /* NOTE K = KiBi = 1024 bytes */
 
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 1024K
  RAM : ORIGIN = 0x20000000, LENGTH = 256K
}



/*
Use the layout below for creating a .elf which when turned into a hex (with the command below) will be able to be burned onto a nrf52840 dongle with the nrf programmer. Nice and easy if you do not need to debug :) 
Also don't forget to adapt the uart ports (in main and serial).
To get a hex file execute the following command in your cargo.toml directory: cargo objcopy --bin rust-jammer --release -- -O ihex output_hex_file.hex 
*/
/*
MEMORY
{
  FLASH (rx) : ORIGIN = 0x1000, LENGTH = 0xff000
  RAM (rwx) :  ORIGIN = 0x20000008, LENGTH = 0x3fff8
}
*/

/* This is where the call stack will be allocated. */
/* The stack is of the full descending type. */
/* You may want to use this variable to locate the call stack and static
   variables in different memory regions. Below is shown the default value */
/* _stack_start = ORIGIN(RAM) + LENGTH(RAM); */

/* You can use this symbol to customize the location of the .text section */
/* If omitted the .text section will be placed right after the .vector_table
   section */
/* This is required only on microcontrollers that store some configuration right
   after the vector table */
/* _stext = ORIGIN(FLASH) + 0x400; */

/* Size of the heap (in bytes) */
/* _heap_size = 1024; */