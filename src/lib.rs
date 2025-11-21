#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]
#![feature(impl_trait_in_assoc_type)]


use esp_alloc::{self as _};
//esp_alloc::heap_allocator!(size: 32 * 1024);
// #[global_allocator]
// static ALLOCATOR: EspHeap = EspHeap::empty();

// Function that can be called by binaries to initialize the heap
pub fn init_heap() {
    // const HEAP_SIZE: usize = 32 * 1024;
    // static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    // unsafe {
    //     esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
    //         HEAP.as_mut_ptr() as *mut u8,
    //         HEAP_SIZE,
    //         esp_alloc::MemoryCapability::Internal.into(),
    //     ));
    // }
    //esp_alloc::heap_allocator!(32 * 1024);
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