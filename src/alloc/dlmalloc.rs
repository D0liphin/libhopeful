use core::ffi::c_size_t;
use std::{
    alloc::{AllocError, Allocator, GlobalAlloc, Layout},
    ffi::c_void,
    ptr::{self, NonNull},
};

use crate::util::{
    assert::{aligned_to, non_null},
    hint::cold,
};

#[link(name = "dlmalloc", kind = "static")]
extern "C" {
    pub(crate) fn dlmalloc(_: c_size_t) -> *mut c_void;

    pub(crate) fn dlfree(_: *mut c_void);

    // TODO: use it
    #[allow(unused)]
    pub(crate) fn dlrealloc(_: *mut c_void, _: c_size_t) -> *mut c_void;

    pub(crate) fn dlmemalign(_: c_size_t, _: c_size_t) -> *mut c_void;
}

/// A wrapper around `dlmalloc()`. Options are configured in
/// `src/clib/dmalloc.c`. Implements both `GlobalAlloc` and `Allocator`. So you
/// should use those as the API.
pub struct DlMalloc(());

impl DlMalloc {
    /// The minimum alignment that `DlMalloc` allocates to
    pub const MIN_ALIGN: usize = 8;

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
        unsafe { dlmalloc(size) }
    }

    /// Safe version of `dlmemalign()`.
    pub(crate) fn dlmemalign(&self, size: c_size_t, align: c_size_t) -> *mut c_void {
        // SAFETY:
        // - Constructor has asserted that `DlMalloc` has complete ownership
        //   over the heap, so this will not interfere with anything else.
        unsafe { dlmemalign(size, align) }
    }

    /// # Safety
    ///
    /// The safety requirements for this function are the same as those for
    /// `dlfree()` or `GlobalAlloc::deallocate()` -- whichever is stricter.
    pub(crate) unsafe fn dlfree(&self, data: *mut c_void) {
        dlfree(data);
    }
}

unsafe impl Allocator for DlMalloc {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let data = if layout.align() <= DlMalloc::MIN_ALIGN {
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

    unsafe fn deallocate(&self, data: NonNull<u8>, layout: Layout) {
        debug_assert!(aligned_to(data.as_ptr(), layout.align()));
        // SAFETY:
        // - Identical contract to caller, which is described in `Allocator`
        //   docs.
        unsafe { self.dlfree(data.as_ptr() as _) };
    }
}

unsafe impl GlobalAlloc for DlMalloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocate(layout).unwrap().as_ptr() as _
    }

    unsafe fn dealloc(&self, data: *mut u8, layout: Layout) {
        debug_assert!(non_null(data));
        // SAFETY: We're totally fine to cast this to non-null, because
        // the caller is required to give a pointer to an allocation made by
        // this allocator. No allocator can return the null-pointer as an
        // allocation!
        self.deallocate(NonNull::new_unchecked(data), layout)
    }
}
