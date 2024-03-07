use std::{
    cell::Cell,
    mem::MaybeUninit,
    ptr::{self, NonNull},
    slice,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::manual::Raw;

pub struct DynArray<T> {
    data: Raw<[MaybeUninit<T>]>,
    len: AtomicUsize,
}

impl<T> DynArray<T> {
    pub fn new() -> Self {
        // TODO: Safety comments for all three unsafe blocks
        let data = ptr::slice_from_raw_parts_mut::<MaybeUninit<T>>(NonNull::dangling().as_ptr(), 0);
        let data = unsafe { NonNull::new_unchecked(data) };
        let data = unsafe { Raw::new(data) };
        Self {
            data,
            len: AtomicUsize::new(0),
        }
    }

    pub fn len(&self) -> usize {
        self.len.load(Ordering::SeqCst)
    }

    // fn realloc(&self) {}
}

/*
I'm trying to make a lock-free `Vec` in Rust. I'm working on reallocation.

Vectors can only grow. We either grow the vector, or free the whole thing.
This observation allows us to make the whole thing lock free.
Normally Vectors have a ptr (in this case called 'data') a length ('len') and a
capacity (in this case stored with 'data', as it is a fat pointer).

I propose adding a new field 'target_len'.
Operations on a fixed-size array are easy to make atomic, doubly so if the array
only contains atomics. Let's go with that assumption for now.

The only thing we need to do to make this a growable array is when we `push()`.
`push()` is defined as

1. Check if the array is the right capacity
2. If it is the wrong capacity, then realloc()
3. Set the value at the offset of length to the value you want

So we can do this atomically if

1. Store old length
2. Allocate a new buffer
3. Swap buffer out with new one
4. Copy over all
*/
