# General programming

## Using collections.

The alloc crate provides a heap implementation.
However you can use vectors, maps,... of a fixed size with th heapless crate!
```rust
extern crate heapless; // v0.4.x

use heapless::Vec;
use heapless::consts::*;

#[entry]
fn main() -> ! {
    let mut xs: Vec<_, U8> = Vec::new();

    xs.push(42).unwrap();
    assert_eq!(xs.pop(), Some(42));
}
```
First, you have to declare upfront the capacity of the collection. heapless collections never reallocate and have fixed capacities; this capacity is part of the type signature of the collection. In this case we have declared that xs has a capacity of 8 elements that is the vector can, at most, hold 8 elements. This is indicated by the U8 (see typenum) in the type signature.

Second, the push method, and many other methods, return a Result. Since the heapless collections have fixed capacity all operations that insert elements into the collection can potentially fail. The API reflects this problem by returning a Result indicating whether the operation succeeded or not. 
That is you'll have deal with all the Results returned by methods like Vec.push.
The alloc API will be familiar to virtually every Rust developer. The heapless API tries to closely mimic the alloc API but it will never be exactly the same due to its explicit error handling -- some developers may feel the explicit error handling is excessive or too cumbersome.

## In lining
The Rust compiler does not by default perform full inlining across crate boundaries. As embedded applications are sensitive to unexpected code size increases, `#[inline]` should be used to guide the compiler as follows:
- All "small" functions should be marked #[inline]. What qualifies as "small" is subjective, but generally all functions that are expected to compile down to single-digit instruction sequences qualify as small.
- Functions that are very likely to take constant values as parameters should be marked as #[inline]. This enables the compiler to compute even complicated initialization logic at compile time, provided the function inputs are known.

## Macros and ifdef compile time code selection
The closest match to #ifdef ... #endif in Rust are Cargo features..

You could declare a Cargo feature for each component in your Cargo.toml:
```rust
[features]
FIR = []
IIR = []
```
Then, in your code, use #[cfg(feature="FIR")] to control what is included.
```rust
/// In your top-level lib.rs

#[cfg(feature="FIR")]
pub mod fir;

#[cfg(feature="IIR")]
pub mod iir;
```
The conditional compilation will only apply to the next statement or block. If a block can not be used in the current scope then the cfg attribute will need to be used multiple times. It's worth noting that most of the time it is better to simply include all the code and allow the compiler to remove dead code when optimising: it's simpler for you and your users, and in general the compiler will do a good job of removing unused code.

Rust supports const fn, functions which are guaranteed to be evaluable at compile-time and can therefore be used where constants are required, such as in the size of arrays. This can be used alongside features mentioned above, for example:
```rust

#![allow(unused)]
fn main() {
const fn array_size() -> usize {
    #[cfg(feature="use_more_ram")]
    { 1024 }
    #[cfg(not(feature="use_more_ram"))]
    { 128 }
}

static BUF: [u32; array_size()] = [0u32; array_size()];
}

```
#### [See this page for macros](https://doc.rust-lang.org/book/ch19-06-macros.html)

## Build.rs
Most Rust crates are built using Cargo (although it is not required). This takes care of many difficult problems with traditional build systems. However, you may wish to customise the build process. Cargo provides build.rs scripts for this purpose. They are Rust scripts which can interact with the Cargo build system as required.

Common use cases for build scripts include:

- provide build-time information, for example statically embedding the build date or Git commit hash into your executable
- generate linker scripts at build time depending on selected features or other logic
- change the Cargo build configuration
- add extra static libraries to link against

At present there is no support for post-build scripts, which you might traditionally have used for tasks like automatic generation of binaries from the build objects or printing build information.

## [This chapter and the following explain how to use C in Rust](https://rust-embedded.github.io/book/interoperability/index.html)


# RTIC
This is like what arduino is supposed to be.
You have an init, an idle (defaults to sleep) and tasks.
You anotate them with the interrupt they fire on, which resource they need and their priority.
You application has 1 resource struct containing all your application resources.
These can be anything, so also my sniffer struct owning the radio pac module.
It provides locks for critical sections in a task based on a resource.
Locking a resource and thus creating a critical section for a resource still allows higher priority tasks which do not need the resource to interrupt. 
If you need code to initialise the resources you need to do it with late resources returned by the init function.
A task by default has &mut access to a resource.
You can specify that all tasks ask & by putting it in front of the resource name on requesting it.
RTIC does not yet support mixing & and &mut, it needs to be one or the other for all tasks.
Tasks can trigger (bind) on an interrupt generated by hardware.
They can also be triggered by software.
You have to declare unused interrupt in an extern block for rtic to use them for your software interrupts.
Because of limited interrupts to reuse, it is smart to group action in one task and choosing them with state or message passing, which is an argument you provide to the task when spawning it in another (MY ARRAY OF MESSAGES!).
You can then spwan them from another task using the context of the task.
If a software task can be in the queue multiple times before it gets executed it needs a "capacity" to reserve static space for all its possible calls at a given moment. If there is no space err() is returned, giving you the chance to recover in a match block, or just unwrap and panic.
RTIC also has a schedule timer queue where you can schedule a task to execute every x seconds. It needs a monotonic timer for this, which rtic provides itself on armv7 with the CYCCNT using the clock cycle counter, however this cannot be used to trigger something more than a couple of seconds away.
This one as well has to be initialised in the init function. (exmaple in the book).
The following code shows what is available from the context by creating a variable _ every time and specifying the type and assigning something the context will hold.
This is the show what you can get from the context and what type it is.

```rust
#[rtic::app(device = lm3s6965, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        #[init(0)]
        shared: u32,
    }

    #[init(schedule = [foo], spawn = [foo])]
    fn init(cx: init::Context) {
        let _: cyccnt::Instant = cx.start;
        let _: rtic::Peripherals = cx.core;
        let _: lm3s6965::Peripherals = cx.device;
        let _: init::Schedule = cx.schedule;
        let _: init::Spawn = cx.spawn;

        debug::exit(debug::EXIT_SUCCESS);
    }

    #[idle(schedule = [foo], spawn = [foo])]
    fn idle(cx: idle::Context) -> ! {
        let _: idle::Schedule = cx.schedule;
        let _: idle::Spawn = cx.spawn;

        loop {}
    }

    #[task(binds = UART0, resources = [shared], schedule = [foo], spawn = [foo])]
    fn uart0(cx: uart0::Context) {
        let _: cyccnt::Instant = cx.start;
        let _: resources::shared = cx.resources.shared;
        let _: uart0::Schedule = cx.schedule;
        let _: uart0::Spawn = cx.spawn;
    }

    #[task(priority = 2, resources = [shared], schedule = [foo], spawn = [foo])]
    fn foo(cx: foo::Context) {
        let _: cyccnt::Instant = cx.scheduled;
        let _: &mut u32 = cx.resources.shared;
        let _: foo::Resources = cx.resources;
        let _: foo::Schedule = cx.schedule;
        let _: foo::Spawn = cx.spawn;
    }

    // RTIC requires that unused interrupts are declared in an extern block when
    // using software tasks; these free interrupts will be used to dispatch the
    // software tasks.
    extern "C" {
        fn SSI0();
    }
};
```

For late initialised resources, they have to implement the send trait. 
Use RefCell for this if it doesn't work from the start.

# Memory and Peripherals

This goes from lower level to high level.
I wrote this to know what to do if there is no preexisting pac or hal or board support.
## [Hardcore write to registers](https://rust-embedded.github.io/book/peripherals/a-first-attempt.html#the-registers)
Map the registers to a struct and force C layout with `#[repr(C)]`, so the fields don't rearrange.
Get a usefull pointer to this `unsafe { &mut *(0xE000_E010 as *mut YourStruct) }` with 0x.. the start address.
To make the compiler do everything you say (volatile in c) for peripheral registers use the `volatile_register::{RW, RO};` crate.
Writes are still unsafe but we cannot do better.
```rust
use volatile_register::{RW, RO};

#[repr(C)]
struct SysTick {
    pub csr: RW<u32>,
    pub rvr: RW<u32>,
    pub cvr: RW<u32>,
    pub calib: RO<u32>,
}

fn get_systick() -> &'static mut SysTick {
    unsafe { &mut *(0xE000_E010 as *mut SysTick) }
}

fn get_time() -> u32 {
    let mut systick = get_systick();
    systick.cvr.read()
    unsafe { systick.rvr.write(234 as u32) }
}
```

Our mut systick variable checks that there are no other references to that particular SystemTimer struct, but they don't stop the user creating a second SystemTimer which points to the exact same peripheral!
That is why you should always use the ``pac/hal::peripheral.take`` which will ensure this.
## [Safe access to peripherals](https://rust-embedded.github.io/book/peripherals/singletons.html#how-do-we-do-this-in-rust)
Instead of just making our peripheral a global variable, we might instead decide to make a global variable, in this case called PERIPHERALS, which contains an Option<T> for each of our peripherals.
This structure allows us to obtain a single instance of our peripheral. If we try to call take_serial() more than once, our code will panic!
This has a small runtime overhead because we must wrap the SerialPort structure in an option, and we'll need to call take_serial() once, however this small up-front cost allows us to leverage the borrow checker throughout the rest of our program.
There are two important factors in play here:

- Because we are using a singleton, there is only one way or place to obtain a SerialPort structure
To call the read_speed() 
- method, we must have ownership or a reference to a SerialPort structure

These two factors put together means that it is only possible to access the hardware if we have appropriately satisfied the borrow checker, meaning that at no point do we have multiple mutable references to the same hardware!
Additionally, because some references are mutable, and some are immutable, it becomes possible to see whether a function or method could potentially modify the state of the hardware.
This allows us to enforce whether code should or should not make changes to hardware at compile time, rather than at runtime.
Rust's type system prevents data races at compile time (see Send and Sync traits). The type system can also be used to check other properties at compile time;
For instance, one can design an API where it is only possible to initialize a serial interface by first configuring the pins that will be used by the interface.
### cortex m singleton
Although we created our own Peripherals structure above, it is not necessary to do this for your code. the cortex_m crate contains a macro called singleton!() that will perform this action for you.
### rtic singleton
If you use cortex-m-rtic, the entire process of defining and obtaining these peripherals are abstracted for you, and you are instead handed a Peripherals structure that contains a non-Option<T> version of all of the items you define.

```rust
// cortex-m-rtic v0.5.x
#[rtic::app(device = lm3s6965, peripherals = true)]
const APP: () = {
    #[init]
    fn init(cx: init::Context) {
        static mut X: u32 = 0;
         
        // Cortex-M peripherals
        let core: cortex_m::Peripherals = cx.core;
        
        // Device specific peripherals
        let device: lm3s6965::Peripherals = cx.device;
    }
}
```

## [Pac, hal or board support crates](https://rust-embedded.github.io/book/start/registers.html)

Click the title to go to a very nice explanation.

## [Using hal crates through the embedded_hal](https://rust-embedded.github.io/book/portability/index.html)

## [Interrupts](https://rust-embedded.github.io/book/start/interrupts.html) & [Exceptions or hard faults](https://rust-embedded.github.io/book/start/exceptions.html) & [panicking](https://rust-embedded.github.io/book/start/panicking.html)


#### [Concurrency (protecting from interrupts)][https://rust-embedded.github.io/book/concurrency/index.html]
Sections below arent really usefull.
The most important thing for me is to use either RTIC or Read very well the following sections https://rust-embedded.github.io/book/concurrency/index.html#mutexes.
## [Critical sections](https://rust-embedded.github.io/book/concurrency/index.html#critical-sections)
If you application has to use a critical section (accessing statics), which should not be interrupted by timers, you can do this as follows:
```rust
static mut COUNTER: u32 = 0;

#[entry]
fn main() -> ! {
    set_timer_1hz();
    let mut last_state = false;
    loop {
        let state = read_signal_level();
        if state && !last_state {
            // New critical section ensures synchronised access to COUNTER
            cortex_m::interrupt::free(|_| {
                unsafe { COUNTER += 1 };
            });
        }
        last_state = state;
    }
}

#[interrupt]
fn timer() {
    unsafe { COUNTER = 0; }
}
```
In this example, we use cortex_m::interrupt::free, but other platforms will have similar mechanisms for executing code in a critical section. 

*This is also the same as disabling interrupts, running some code, and then re-enabling interrupts.*

Note we didn't need to put a critical section inside the timer interrupt, for two reasons:
- Writing 0 to COUNTER can't be affected by a race since we don't read it
- It will never be interrupted by the main thread anyway

If COUNTER was being shared by multiple interrupt handlers that might preempt each other, then each one might require a critical section as well.

Since each critical section temporarily pauses interrupt processing, there is an associated cost of some extra code size and higher interrupt latency and jitter (interrupts may take longer to be processed, and the time until they are processed will be more variable). Whether this is a problem depends on your system, but in general, we'd like to avoid it.

It's worth noting that while a critical section guarantees no interrupts will fire, it does not provide an exclusivity guarantee on multi-core systems! The other core could be happily accessing the same memory as your core, even without interrupts. You will need stronger synchronisation primitives if you are using multiple cores.

## [Atomic access](https://rust-embedded.github.io/book/concurrency/index.html#atomic-access) 

On some platforms, atomic instructions are available, which provide guarantees about read-modify-write operations. Specifically for Cortex-M, thumbv6 (Cortex-M0) does not provide atomic instructions, while thumbv7 (Cortex-M3 and above) do. These instructions give an alternative to the heavy-handed disabling of all interrupts: we can attempt the increment, it will succeed most of the time, but if it was interrupted it will automatically retry the entire increment operation. These atomic operations are safe even across multiple cores.
```rust
//... same
    // Use `fetch_add` to atomically add 1 to COUNTER
            COUNTER.fetch_add(1, Ordering::Relaxed);
//... same
// Use `store` to write 0 directly to COUNTER
    COUNTER.store(0, Ordering::Relaxed)
```