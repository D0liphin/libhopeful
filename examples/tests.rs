//! For now, tests are going to go here, because apparently I can't set a
//! global allocator in a test... who knew!?

#![feature(allocator_api)]

use lhl::alloc::{dlmalloc::DlMalloc, tracing::TracingAlloc};

#[global_allocator]
static GLOBAL: TracingAlloc<DlMalloc> =
unsafe { TracingAlloc::new(DlMalloc::new(), DlMalloc::new()) };

fn main() {
    let buf = vec![0u8; 220];
    for ptr in [&buf[45], 56usize as *const u8, &buf[0]] {
        if let Some(alloc_id) = GLOBAL.find(ptr) {
            let offset = ptr as usize - alloc_id.ptr as usize;
            println!("ptr is part of allocation {alloc_id:?}, offset by {offset} bytes");
        } else {
            println!("ptr is not part of any allocation");
        }
    }
}
