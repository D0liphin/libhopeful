//! For now, tests are going to go here, because apparently I can't set a
//! global allocator in a test... who knew!?

#![feature(allocator_api)]

use lhl::alloc::{dlmalloc::DlMalloc, tracing::TracingAlloc};

#[global_allocator]
static GLOBAL: TracingAlloc<DlMalloc> =
    unsafe { TracingAlloc::new(DlMalloc::new(), DlMalloc::new()) };

fn main() {
    let buf = vec![Box::new(0); 220];
    for ptr in [
        buf[45].as_ref() as *const _ as usize,
        56usize,
        buf[0].as_ref() as *const _ as usize + 3,
    ] {
        if let Some(alloc_id) = GLOBAL.find(ptr as *const ()) {
            let offset = ptr - alloc_id.ptr as usize;
            println!("ptr is part of allocation {alloc_id:?}, offset by {offset} bytes");
        } else {
            println!("ptr is not part of any allocation");
        }
    }
    println!(
        "there are currently {} allocations active",
        GLOBAL.nr_allocations()
    );
}


