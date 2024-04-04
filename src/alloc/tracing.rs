//! A wrapper for allocator `malloc()` that holds additional meta information
//! for each allocation.
//!
//! First, we need to check if something *is* a heap allocation. We can't just
//! check the header, because we are not guaranteed to be able to offset a
//! pointer backwards *by any amount*, and also how do we know that some
//! arbitrary bytes even *are* a header??? So we need to have some kind of
//! global data structure. We also need that structure to be thread safe. This
//! structure can also hold a fn pointer (since we're going to need to choke the
//! cache miss anyway, we may as well take the whole line).
//!
//! The thing is, we're going to need to tag the allocations with more type
//! information *post-hoc*. Which is not ideal. Let's see what we can do without
//! that information:
//!
//! 1. We can scan the struct for pointers that we monitor. This requires
//!    banning
//! 2.
//!
//! ```no_run
//! let n = 5;
//! ```

use ahash::AHasher;
use hashbrown::{hash_map::DefaultHashBuilder, HashMap};

use crate::{
    alloc::manual::AllocatorExt,
    arch::mem::{memcpy_maybe_garbage, usize_load_acq},
    fcprintln, put, putln,
    util::print::{cout, endl, putstr, Bits},
};
use std::{
    alloc::{Allocator, GlobalAlloc, Layout},
    cmp, mem,
    ptr::{self, NonNull},
    sync::atomic::AtomicPtr,
};

use super::{
    bitvec::BitMap,
    dlmalloc::{DlMalloc, SBRK_START},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocId {
    pub ptr: *const (),
    pub size: usize,
}

pub struct AllocMeta {
    layout: Layout,
}

/// # Safety
/// - Must return allocations at least 8-byte aligned
pub unsafe trait FlatAllocator {
    /// Returns the minimum (never changing) vaddr that is valid for reads in
    /// this allocator's arena
    fn min_vaddr(&self) -> usize;
}

// SAFETY: dlmalloc() returns 16-byte aligned chunks
unsafe impl FlatAllocator for DlMalloc {
    fn min_vaddr(&self) -> usize {
        *SBRK_START
    }
}

/// Additional metadata that every allocation by [`TracingAlloc`] has
#[repr(C)]
pub struct TracingAllocHeader {
    /// The exact requested size of this allocation
    size: usize,
    /// (Currently unused) metadata about this allocation -- 'static
    metadata: AtomicPtr<()>,
}

impl TracingAllocHeader {
    /// Get the layout for a given allocation, as well as the size of the header
    /// This will let you determine at what offset the returned pointer should
    /// be, the layout of a `TracingAlloc` allocation currently looks like this
    ///
    /// ```plaintext
    /// +--------------------+
    /// | dlmalloc header    |
    /// +--------------------+
    /// | padding...         | header_size bytes
    /// | TracingAllocHeader |
    /// +--------------------+
    /// | actual requested   | tracing_alloc_header.size bytes
    /// | size               |
    /// +--------------------+
    /// | dlmalloc stuff     |
    /// .                    .
    /// ```
    pub fn layout_with_header(layout: Layout) -> (usize, Layout) {
        let header_size = cmp::max(mem::size_of::<Self>(), layout.align());
        let layout = unsafe {
            Layout::from_size_align_unchecked(layout.size() + header_size, layout.align())
        };
        (header_size, layout)
    }

    pub const fn empty() -> Self {
        Self {
            size: 0,
            metadata: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

/// An allocator, which allows for GC-style tracing of allocations, from a
/// specified root. The allocator is backed by `A`, which should be an allocator
/// that uses an arena that grows linearly with the number of bytes allocated,
/// since the metadata grows linearly with `arena_size` where
/// `arena_size = max_vaddr - MIN_VADDR`.
///
/// # Safety
///
/// - `MIN_VADDR` must never decrease
pub struct TracingAlloc<A>
where
    A: Allocator,
{
    allocator: A,
    bit_map: BitMap<A>,
}

impl<A> TracingAlloc<A>
where
    A: Allocator + FlatAllocator,
{
    /// TODO: Safety
    pub const unsafe fn new(allocator: A, the_same_allocator: A) -> Self {
        Self {
            allocator,
            bit_map: BitMap::new(the_same_allocator),
        }
    }

    fn get_bit_map_index<T>(&self, ptr: *const T) -> Option<usize> {
        if (ptr as usize) < self.allocator().min_vaddr() {
            return None;
        }
        let bmi = ptr as usize - self.allocator().min_vaddr();
        Some(bmi)
    }

    pub fn find<T>(&self, ptr: *const T) -> Option<AllocId> {
        let bit_map_index = self.get_bit_map_index(ptr)?;
        match self.bit_map.get(bit_map_index).scan_backward() {
            Some(steps) => {
                // TODO: verify that this matches _every_ case
                let alloc_ptr = (ptr as *const u8).wrapping_offset(-(steps as isize));
                let header_ptr = alloc_ptr as usize - mem::size_of::<TracingAllocHeader>();
                let mut header = TracingAllocHeader::empty();
                // TODO: Safety
                unsafe {
                    memcpy_maybe_garbage(
                        &mut header as *mut TracingAllocHeader as *mut u8,
                        header_ptr as *const u8,
                        mem::size_of::<TracingAllocHeader>(),
                    )
                };
                if steps >= header.size {
                    None
                } else {
                    Some(AllocId {
                        ptr: alloc_ptr as _,
                        size: header.size,
                    })
                }
            }
            None => None,
        }
    }
}
impl<A> TracingAlloc<A>
where
    A: Allocator,
{
    /// Return the underlying allocator
    pub const fn allocator(&self) -> &A {
        &self.allocator
    }
}

unsafe impl<A> GlobalAlloc for TracingAlloc<A>
where
    A: Allocator + FlatAllocator,
{
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let header = TracingAllocHeader {
            metadata: AtomicPtr::new(ptr::null_mut()),
            size: layout.size(),
        };

        let (header_size, layout) = TracingAllocHeader::layout_with_header(layout);
        putln!(
            "alloc(size: ",
            layout.size(),
            ", align: ",
            layout.align(),
            ", header_size: ",
            header_size,
            ")"
        );
        let data = self.allocator().xallocate(layout);

        // SAFETY: This is always going to be aligned for dlmalloc, but we don't
        //         know that this is dlmalloc!
        unsafe {
            data.cast::<u8>()
                .add(header_size - mem::size_of::<TracingAllocHeader>())
                .cast::<TracingAllocHeader>()
                .write_unaligned(header)
        }

        // SAFETY: we know this to be within the bounds of the allocation (or
        //         one past the end).
        let mut data = unsafe { data.cast::<u8>().add(header_size) };
        debug_assert!(data.is_aligned_to(layout.align()));

        let index = self
            .get_bit_map_index(data.as_mut())
            .expect("Any valid allocation should be greater than min_vaddr");
        self.bit_map.set_high(index);

        data.as_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // Assertions made by caller (e.g. null cannot be 'allocated via this
        // allocator... I think)
        debug_assert!(ptr.is_aligned_to(layout.align()));
        debug_assert!(!ptr.is_null());

        let (header_size, layout) = TracingAllocHeader::layout_with_header(layout);
        // SAFETY: Caller asserts this is a valid pointer allocated by this
        //         flat allocator, so `ptr` must be in the allocated region
        let index = unsafe { self.get_bit_map_index(ptr).unwrap_unchecked() };
        self.bit_map.set_low(index);
        self.allocator().deallocate(
            unsafe { NonNull::new_unchecked(ptr).offset(-(header_size as isize)) },
            layout,
        );
    }
}
