#![allow(internal_features)]
#![feature(
    core_intrinsics,
    allocator_api,
    lazy_cell,
    c_size_t,
    unchecked_math,
    alloc_layout_extra,
    const_slice_from_raw_parts_mut,
    slice_ptr_len,
    slice_ptr_get,
    non_null_convenience,
    pointer_is_aligned
)]

pub mod alloc;
mod lazy_lock;
pub mod util;
pub mod arch;

#[derive(Debug)]
pub struct Foo {
    pub a: i32,
}
