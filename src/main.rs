#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(cos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use cos::println;
use cos::task::{executor::Executor, keyboard, Task};
use cos::ata;
use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use cos::allocator;

    println!("Hello World{}", "!");

    cos::init(boot_info);

    #[cfg(test)]
    test_main();

    let mut executor = Executor::new();
    executor.spawn(Task::new(keyboard::print_keypresses()));
    executor.run();
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    cos::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    cos::test_panic_handler(info)
}

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}
