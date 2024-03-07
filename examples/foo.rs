// #![feature(allocator_api)]
// use lhl::alloc::{global::TracingAlloc, meta::Address};

// #[global_allocator]
// static ALLOC: TracingAlloc = TracingAlloc;

// /*
// CURRENT ISSUE:
// We cannot do anything, because the `ALLOC_META_INDEX` needs to allocate...
// What's the issue though? Well... let's say we allocate
// */

// fn main() {
//     let b = Box::new(42);
//     println!("Some boxed int = {}", b);
//     println!("  Metadata = {:?}", b.as_ref().alloc_meta());
// }


fn main() {}