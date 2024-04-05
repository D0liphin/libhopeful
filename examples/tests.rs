//! For now, tests are going to go here, because apparently I can't set a
//! global allocator in a test... who knew!?

#![feature(allocator_api)]

use lhl::alloc::{
    dlmalloc::DlMalloc,
    tracing::{AllocId, FindPointerMethod, TracingAlloc},
};

#[global_allocator]
static GLOBAL: TracingAlloc<DlMalloc> =
    unsafe { TracingAlloc::new(DlMalloc::new(), DlMalloc::new()) };

fn main() {
let buf = [
    Some(Box::new(0x12345678u32)),
    None,
    Some(Box::new(0xabababab)),
    None,
    Some(Box::new(0xcececece)),
];
let alloc_id = AllocId::from_sized(&buf);
let ptrs = alloc_id.find_pointers(&GLOBAL, FindPointerMethod::AlignedHeapOnly);
for ptr in ptrs {
    println!("contains ptr --> {:?}", unsafe { ptr.id.read_unchecked() });
}
}
