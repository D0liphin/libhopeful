use core::ffi::c_size_t;
use std::{
    alloc::{AllocError, Allocator, GlobalAlloc, Layout},
    ffi::c_void,
    mem::align_of,
    ptr::{self, NonNull},
};

use crate::util::hint::cold;

// https://gee.cs.oswego.edu/dl/html/malloc.html

pub const DLMALLOC_MIN_ALIGN: usize = 8;

#[link(name = "dlmalloc", kind = "static")]
extern "C" {
    pub(crate) fn malloc(_: c_size_t) -> *mut c_void;

    pub(crate) fn free(_: *mut c_void);

    // TODO: use it
    #[allow(unused)]
    pub(crate) fn realloc(_: *mut c_void, _: c_size_t) -> *mut c_void;

    pub(crate) fn memalign(_: c_size_t, _: c_size_t) -> *mut c_void;
}

/// A wrapper around `dlmalloc()`. Options are configured in
/// `src/clib/dmalloc.c`. Implements both `GlobalAlloc` and `Allocator`. So you
/// should use those as the API.
pub struct DlMalloc(());

impl DlMalloc {
    /// Construct a new `DlMalloc` allocator. This is a no-op.
    ///
    /// # Safety
    ///
    /// To scope the unsafe to this function call, you must guarantee that you
    /// have no other allocators that might assume ownership of the same heap
    /// as `dlmalloc()`. So, really that means a global allocator that is not
    /// `DlMalloc`, or any other general purpose heap allocator, really.
    ///
    /// Alternatively, you could just *not use* those allocators ever again, but
    /// that's harder to get right.
    pub const unsafe fn new() -> Self {
        Self(())
    }

    /// Safe version of `dlmalloc()`. `dlfree()` has no safe counterpart.
    pub(crate) fn dlmalloc(&self, size: c_size_t) -> *mut c_void {
        // SAFETY:
        // - Constructor has asserted that `DlMalloc` has complete ownership
        //   over the heap, so this will not interfere with anything else.
        unsafe { malloc(size) }
    }

    /// Safe version of `dlmemalign()`.
    pub(crate) fn dlmemalign(&self, size: c_size_t, align: c_size_t) -> *mut c_void {
        // SAFETY:
        // - Constructor has asserted that `DlMalloc` has complete ownership
        //   over the heap, so this will not interfere with anything else.
        unsafe { memalign(size, align) }
    }

    /// # Safety
    /// 
    /// The safety requirements for this function are the same as those for 
    /// `dlfree()` or `GlobalAlloc::deallocate()` -- whichever is stricter.
    pub(crate) unsafe fn dlfree(&self, data: *mut c_void) {
        free(data);
    }
}

unsafe impl Allocator for DlMalloc {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let data = if layout.align() <= DLMALLOC_MIN_ALIGN {
            self.dlmalloc(layout.size())
        } else {
            cold(|| self.dlmemalign(layout.size(), layout.align()))
        };

        if !data.is_null() {
            Ok(cold(|| {
                let slice = ptr::slice_from_raw_parts_mut(data as _, layout.size());
                NonNull::new(slice).expect("just asserted non-null")
            }))
        } else {
            Err(AllocError)
        }
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, _: Layout) {
        // SAFETY:
        // - Identical contract to caller, which is described in `Allocator`
        //   docs.
        unsafe { self.dlfree(ptr.as_ptr() as _) };
    }
}

unsafe impl GlobalAlloc for DlMalloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocate(layout).unwrap().as_ptr() as _
    }

    unsafe fn dealloc(&self, data: *mut u8, layout: Layout) {
        // TODO: add debug assertions

        // SAFETY: We're totally fine to cast this to non-null, because
        // the caller is required to give a pointer to an allocation made by
        // this allocator. No allocator can return the null-pointer as an
        // allocation!
        unsafe { self.deallocate(NonNull::new_unchecked(data), layout) }
    }
}
