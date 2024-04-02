#![allow(internal_features)]
#![feature(
    core_intrinsics,
    allocator_api,
    lazy_cell,
    c_size_t,
    unchecked_math,
    alloc_layout_extra,
    const_slice_from_raw_parts_mut,
    slice_ptr_len
)]

pub mod alloc;
mod lazy_lock;
pub mod util;

#[derive(Debug)]
pub struct Foo {
    pub a: i32,
}
