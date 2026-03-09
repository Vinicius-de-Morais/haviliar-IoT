#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]
#![feature(impl_trait_in_assoc_type)]


use esp_alloc as _;

// Function that can be called by binaries to initialize the heap
// esp-alloc automatically initializes the heap when imported
pub fn init_heap() {
    // Heap is automatically initialized by esp-alloc import above
}


pub mod hal;
pub mod factory;
extern crate alloc;
pub mod controller;

// Add this to your lib.rs file
#[cfg(test)]
pub fn test_runner(tests: &[&dyn Fn()]) {
    // Minimal implementation
    for test in tests {
        test();
    }
}

// Also add a panic handler for test mode
#[cfg(test)]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}