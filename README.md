# rust\_jammer
Trying to get a nrf52840 jammer in rust.

## toolchain
### Rust
Install rust from the rust website.
Add the m4f architecture `rustup target add thumbv7em-none-eabihf`.

To build the source code, execute `cargo build`.
The binary file for the chip is now located in the target/thumbv7em-none-eabihf/debug/ directory and the binary has the same name as this project, namely rust-jammer.

### Debugging
JLink is used for debugging.
It is a more expensive version by Segger of the STLink JTag tool.
The pins go into the board, which returns over usb a JLINK protocol for commanding the cpu.
Segger has a lot of downloads to use all the tool available for this [here](https://www.segger.com/downloads/jlink/#J-LinkSoftwareAndDocumentationPack).
One of the things it can do is create a gdb server, to which you can connect with a normal GDB installation.
It has extra functions such as halting the target processor.

Starting the gdb server on linux after installing the toolchain: 
`JLinkGDBServer -if swd -device nrf52840_xxaa`.
To see the debug output (over RTT) of the chip in a terminal, use the 
`JLinkRTTClient` command.

Make sure you have installed the multiarch package of gdb `sudo apt install gdb-multiarch`.
After this, you can connect to it with GDB by executing `gdb-multiarch` and providing the following in the shell:
```
target remote localhost:2331
file <path to binary (elf,hex,...)>
monitor reset
load
```
Now you can use gdb as normal, setting breakpoints like `break main` and `continue`.

However, there are quite a lot of commands to pass to gdb, so a .gdb file has been provided to easily execute gdb with `gdb-multiarch -q -x jlink.gdb` from the root directory of the project.
This will drop you at the start of the execution of the binary, halted.