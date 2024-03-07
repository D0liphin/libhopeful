#![feature(allocator_api)]

use std::alloc::GlobalAlloc;
use std::io::Write;

struct PanickingAlloc;

unsafe impl GlobalAlloc for PanickingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        unimplemented!()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unimplemented!()
    }
}

#[global_allocator]
static GLOBAL: PanickingAlloc = PanickingAlloc;

fn main() {
    _ = writeln!(std::io::stdout().lock(), "Hello, world!");
}
