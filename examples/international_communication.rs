use std::{
    mem::ManuallyDrop,
    ptr::{null, null_mut},
};

use lhl::{
    alloc::{
        dlmalloc::DlMalloc,
        tracing::{FindPointerMethod, TracingAlloc},
    },
    externc::LHLMALLOC,
    graph::AllocGraph,
};
use libc::c_int;

#[global_allocator]
static GLOBAL: &TracingAlloc<DlMalloc> = &LHLMALLOC;

#[repr(C)]
struct Node {
    next: *const Node,
    value: c_int,
}

extern "C" {
    fn some_linked_list() -> *mut Node;
}

fn raw_alloc<T>(value: T) -> *mut T {
    ManuallyDrop::new(Box::new(value)).as_mut() as *mut T
}

fn main() {
let dst = raw_alloc(Node {
    next: unsafe { some_linked_list() },
    value: 5,
});
    AllocGraph::from_sized(&GLOBAL, &dst, FindPointerMethod::AlignedHeapOnly);
}
