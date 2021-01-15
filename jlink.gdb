# set gdb behaviour
set pagination off
set history save on
set history filename .gdb-history
set history size 1000

# connect to file
file target/thumbv7em-none-eabihf/debug/rust-jammer
target remote localhost:2331
monitor reset

# print demangled symbols
set print asm-demangle on

# set backtrace limit to not have infinite backtrace loops
set backtrace limit 32

# detect unhandled exceptions, hard faults and panics
# the default function that gets called on an exception like systick, expeption handles cannot be called by the user. You can create your own with the #[exception]
break DefaultHandler 
# Gets called when you really F up and call to a nonexistent (not mapped) memroy address or something like that
break HardFault 
# causes the debugging to halt when panicking, because panic_halt as _ calls this function when panicking.. The nrf has its own bkpt instructions. TODO lookin to this
break rust_begin_unwind 

# *try* to stop at the user entry point (it might be gone due to inlining)
break main

# Semihosting is a debugging tool that is built in to most ARM-based microcontrollers. 
# It allows you to use input and output functions on a host computer that get forwarded to your microcontroller over a hardware debugging tool (e.g. ST-LINK). 
# Specifically, it allows you to use printf() debugging to output strings to your IDE’s console while running code on the microcontroller. 
# Note that we’ll be doing this over the Serial Wire Debug (SWD) port.
#monitor arm semihosting enable
# CHANGED to jlink equivalent
#monitor semihosting enable
# dont know where to view, used rtt instead.

load

# start the process but immediately halt the processor
stepi
#continue
