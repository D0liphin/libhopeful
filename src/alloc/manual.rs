use std::{
    alloc::{AllocError, Layout},
    mem::MaybeUninit,
    ptr::{self, NonNull},
};

use crate::util::hint::cold;

pub(crate) mod c {
    use core::ffi::{c_size_t, c_void};

    extern "C" {
        pub fn malloc(size: c_size_t) -> *mut c_void;
        pub fn free(ptr: *mut c_void);
    }
}

/// This is just a regular allocator function, without any metadata stored
pub fn malloc(layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
    let data = unsafe { c::malloc(layout.size() as _) };
    if data.is_null() {
        return cold(|| Err(AllocError));
    }
    let data = data as *mut u8;
    let data = ptr::slice_from_raw_parts_mut(data, layout.size());
    // SAFETY:
    //   - Pointer is unique and `SharedReadWrite`, so writes are valid
    //   - `malloc()` guarantees pointer to point to at least a `layout.size()`
    //     sized object
    unsafe {
        (data as *mut u8).write_bytes(0, layout.size());
    }
    Ok(NonNull::new(data).expect("just asserted that the pointer is non-null"))
}

/// An initialized, owning pointer to a `T`, that has no RAII -- you must free
/// it yourself, using `dealloc()`.
pub struct Raw<T>
where
    T: ?Sized,
{
    data: NonNull<T>,
}

impl<T> Raw<T>
where
    T: ?Sized,
{
    /// # Safety
    ///
    /// - You must create an owning pointer here. Many of [`Raw`]'s methods'
    ///   soundness rely on the fact that it is unique.
    /// - The pointer must be initialized
    pub unsafe fn new(data: NonNull<T>) -> Self {
        Self { data }
    }

    pub fn leak(self) -> &'static T {
        // SAFETY: This is a unique pointer, so it won't conflict with any other
        //         references.
        unsafe { &*self.data.as_ptr() }
    }

    pub fn free(self) {
        // SAFETY:
        // - This is a unique pointer, so we can totall free it safely, by the
        //   same logic as [`std::boxed::Box<T>`]
        // - Of course, this is also non-null.
        unsafe { free(self.data.as_ptr() as *mut u8) }
    }

    pub const fn as_ptr(&self) -> *mut T {
        self.data.as_ptr()
    }
}

/// Allocate and initialize a `T`, panicking if there is insufficient memory
/// or some other allocation error. It is safe to cast the returned pointer to
/// pretty much any pointer type you like...
///
/// TODO: fix the `.expect("insufficient memory")` rubbish
pub fn xmalloc<T>(val: T) -> Raw<T> {
    let data = malloc(Layout::new::<T>()).expect("insufficient memory");
    // SAFETY: Asserted by `alloc` -- the resulting pointer can
    //         be cast to a type with the requested `Layout`.
    let data: NonNull<T> = data.cast();
    // SAFETY:
    //   - `data` is the only pointer to this location
    //   - See previous comment for cast safety
    unsafe {
        ptr::write(data.as_ptr(), val);
    }
    // SAFETY:
    //   - `data` is unique
    //   - `data` is init
    unsafe { Raw::new(data) }
}

fn buf_layout<T>(count: usize) -> Layout {
    let (layout, _) = Layout::new::<T>()
        .repeat(count)
        .expect("insufficient memory");
    layout
}

/// Allocates a buffer of `count` instances of `T`... All values remain uninit,
/// so you should initialize them all yourself, before casting to a more useful
/// type.
pub fn xbufalloc<T>(count: usize) -> Raw<[MaybeUninit<T>]> {
    let layout = buf_layout::<T>(count);
    let data = malloc(layout).expect("insufficient memory");
    // SAFETY:
    // - Is non-null (if it were null, we would have panicked after `malloc()`)
    // - Is valid for arrays of T, of length `count`, due to allocating with the
    //   above layout.
    let data = unsafe {
        NonNull::new_unchecked(ptr::slice_from_raw_parts_mut(
            data.as_ptr() as *mut T as *mut MaybeUninit<T>,
            count,
        ))
    };
    // SAFETY: Is unique
    unsafe { Raw::new(data) }
}

/// # Safety
///
/// - Since we assume that any registered pointer is valid for reads, a call to
///   this on a pointer with metadata, should be followed by `remove_meta()` to
///   unregister the pointer.
pub unsafe fn free(data: *mut u8) {
    unsafe { c::free(data as _) }
}

/// Like `free()` buf works with `Raw<T>`
pub fn xfree<T>(data: Raw<T>)
where
    T: ?Sized,
{
    // SAFETY:
    //   - Pointer is unique
    //   - So freeing it cannot invalidate invariants of any other pointer
    unsafe { free(data.as_ptr() as *mut u8) }
}

/// Reallocate a buffer, copying the original contents over
pub fn xbufrealloc<T>(data: Raw<[MaybeUninit<T>]>, count: usize) -> Raw<[MaybeUninit<T>]> {
    let new_data = xbufalloc::<T>(count);
    // SAFETY:
    //   - `data` is unique and should be `SharedReadWrite`
    //   - `new_data` is the same (so both sides are valid for both reads and
    //     writes)
    //   - `new_data` has been allocated with the same `buf_layout` as specified
    //   - Nonoverlapping because of allocator requirement
    //   - I think this is technically UB because `T` might be an `&mut U`,
    //     though this should never have an actual effect (TODO: verify).
    unsafe {
        ptr::copy_nonoverlapping(
            data.as_ptr() as *const T,
            new_data.as_ptr() as *mut T,
            buf_layout::<T>(count).size(),
        );
    }
    xfree(data);
    new_data
}


// tim harris linked list lock free