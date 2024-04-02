use std::{
    alloc::{AllocError, Allocator, Layout},
    borrow::{Borrow, BorrowMut},
    mem::{self, MaybeUninit},
    ptr::{self, NonNull},
    slice,
};

use crate::util::{assert::non_null, hint::cold};

// pub(crate) mod c {
//     use core::ffi::{c_size_t, c_void};

//     extern "C" {
//         pub fn malloc(size: c_size_t) -> *mut c_void;
//         pub fn free(ptr: *mut c_void);
//     }
// }

// /// This is just a regular allocator function, without any metadata stored
// pub fn malloc(layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
//     let data = unsafe { c::malloc(layout.size() as _) };
//     if data.is_null() {
//         return cold(|| Err(AllocError));
//     }
//     let data = data as *mut u8;
//     let data = ptr::slice_from_raw_parts_mut(data, layout.size());
//     // SAFETY:
//     //   - Pointer is unique and `SharedReadWrite`, so writes are valid
//     //   - `malloc()` guarantees pointer to point to at least a `layout.size()`
//     //     sized object
//     unsafe {
//         (data as *mut u8).write_bytes(0, layout.size());
//     }
//     Ok(NonNull::new(data).expect("just asserted that the pointer is non-null"))
// }

// /// An initialized, owning pointer to a `T`, that has no RAII -- you must free
// /// it yourself, using `dealloc()`.
// #[repr(transparent)]
// pub struct Raw<T>
// where
//     T: ?Sized,
// {
//     data: NonNull<T>,
// }

// impl<T> Raw<T>
// where
//     T: ?Sized,
// {
//     /// # Safety
//     ///
//     /// - You must create an owning pointer here. Many of [`Raw`]'s methods'
//     ///   soundness rely on the fact that it is unique.
//     /// - The pointer must be initialized
//     pub const unsafe fn new(data: NonNull<T>) -> Self {
//         Self { data }
//     }

//     pub fn leak(self) -> &'static T {
//         // SAFETY: This is a unique pointer, so it won't conflict with any other
//         //         references.
//         unsafe { &*self.data.as_ptr() }
//     }

//     pub fn free(self) {
//         // SAFETY:
//         // - This is a unique pointer, so we can totall free it safely, by the
//         //   same logic as [`std::boxed::Box<T>`]
//         // - Of course, this is also non-null.
//         unsafe { free(self.data.as_ptr() as *mut u8) }
//     }

//     pub const fn as_ptr(&self) -> *mut T {
//         self.data.as_ptr()
//     }
// }

// impl<T> Raw<[T]> {
//     /// Construct an empty slice.
//     pub const fn empty_slice() -> Raw<[T]> {
//         // SAFETY: pointer is uniquely owned by `Raw`
//         unsafe { Raw::new(NonNull::slice_from_raw_parts(NonNull::dangling(), 0)) }
//     }
// }

// /// Allocate and initialize a `T`, panicking if there is insufficient memory
// /// or some other allocation error. It is safe to cast the returned pointer to
// /// pretty much any pointer type you like...
// ///
// /// TODO: fix the `.expect("insufficient memory")` rubbish
// pub fn xmalloc<T>(val: T) -> Raw<T> {
//     let data = malloc(Layout::new::<T>()).expect("insufficient memory");
//     // SAFETY: Asserted by `alloc` -- the resulting pointer can
//     //         be cast to a type with the requested `Layout`.
//     let data: NonNull<T> = data.cast();
//     // SAFETY:
//     //   - `data` is the only pointer to this location
//     //   - See previous comment for cast safety
//     unsafe {
//         ptr::write(data.as_ptr(), val);
//     }
//     // SAFETY:
//     //   - `data` is unique
//     //   - `data` is init
//     unsafe { Raw::new(data) }
// }

// fn buf_layout<T>(count: usize) -> Layout {
//     let (layout, _) = Layout::new::<T>()
//         .repeat(count)
//         .expect("insufficient memory");
//     layout
// }

// /// Allocates a buffer of `count` instances of `T`... All values remain uninit,
// /// so you should initialize them all yourself, before casting to a more useful
// /// type.
// pub fn xbufalloc<T>(count: usize) -> Raw<[MaybeUninit<T>]> {
//     let layout = buf_layout::<T>(count);
//     let data = malloc(layout).expect("insufficient memory");
//     // SAFETY:
//     // - Is non-null (if it were null, we would have panicked after `malloc()`)
//     // - Is valid for arrays of T, of length `count`, due to allocating with the
//     //   above layout.
//     let data = unsafe {
//         NonNull::new_unchecked(ptr::slice_from_raw_parts_mut(
//             data.as_ptr() as *mut T as *mut MaybeUninit<T>,
//             count,
//         ))
//     };
//     // SAFETY: Is unique
//     unsafe { Raw::new(data) }
// }

// /// # Safety
// ///
// /// - Since we assume that any registered pointer is valid for reads, a call to
// ///   this on a pointer with metadata, should be followed by `remove_meta()` to
// ///   unregister the pointer.
// pub unsafe fn free(data: *mut u8) {
//     unsafe { c::free(data as _) }
// }

// /// Like `free()` buf works with `Raw<T>`
// pub fn xfree<T>(data: Raw<T>)
// where
//     T: ?Sized,
// {
//     // SAFETY:
//     //   - Pointer is unique
//     //   - So freeing it cannot invalidate invariants of any other pointer
//     unsafe { free(data.as_ptr() as *mut u8) }
// }

// /// Reallocate a buffer, copying the original contents over
// ///
// /// # Safety
// /// - See `xbufrealloc_in_place()`
// pub unsafe fn xbufrealloc<T>(data: Raw<[MaybeUninit<T>]>, count: usize) -> Raw<[MaybeUninit<T>]> {
//     let new_data = xbufalloc::<T>(count);
//     let count = data.as_ptr().len();
//     unsafe { xbufrealloc_in_place(data, count, new_data) }
// }

// /// Reallocate a buffer, copying the original contents over, but provide with
// /// a buffer that is at least `count`
// ///
// /// # Safety
// /// - We attempt to copy `count` bytes from `data` to `new_data`, so both must
// ///   be at least `count` bytes long. You could realloc() a smaller buffer, we
// ///   just chop the end off.
// pub unsafe fn xbufrealloc_in_place<T>(
//     data: Raw<[MaybeUninit<T>]>,
//     count: usize,
//     new_data: Raw<[MaybeUninit<T>]>,
// ) -> Raw<[MaybeUninit<T>]> {
//     debug_assert!(count <= new_data.as_ptr().len());
//     debug_assert!(count <= data.as_ptr().len());
//     // SAFETY:
//     //   - `data` is unique and should be `SharedReadWrite`
//     //   - `new_data` is the same (so both sides are valid for both reads and
//     //     writes)
//     //   - `new_data` has been allocated with the same `buf_layout` as specified
//     //   - Nonoverlapping because of allocator requirement
//     //   - I think this is technically UB because `T` might be an `&mut U`,
//     //     though this should never have an actual effect (TODO: verify).
//     unsafe {
//         ptr::copy_nonoverlapping(
//             data.as_ptr() as *const T,
//             new_data.as_ptr() as *mut T,
//             buf_layout::<T>(count).size(),
//         );
//     }
//     xfree(data);
//     new_data
// }

// ---

pub trait LayoutExt {
    /// Create a new layout for a slice `[T]` that can hold `count` elements.
    ///
    /// # Panics
    ///
    /// If an impossible layout has been requested
    fn new_slice<T>(count: usize) -> Layout {
        Layout::new::<T>()
            .repeat(count)
            .expect("invalid layout requested")
            .0
    }
}

impl LayoutExt for Layout {}

/// An initialized, owning pointer to a `T`, that has no RAII -- you must free
/// it yourself, using `dealloc()`.
pub struct Unique<T, A>
where
    T: ?Sized,
    A: Allocator,
{
    data: NonNull<T>,
    allocator: A,
}

impl<T, A> Unique<T, A>
where
    T: ?Sized,
    A: Allocator,
{
    /// Allocate a new spot for a `U`. `U` must be sized, for unsized types, you
    /// might be able to use something like [`Unique::new_slice`].
    ///
    /// To safely initialize this type, use `Unique::init`
    pub fn new<U>(allocator: A) -> Unique<MaybeUninit<U>, A> {
        Unique {
            data: allocator
                .xallocate(Layout::new::<U>())
                .cast::<MaybeUninit<U>>(),
            allocator,
        }
    }

    pub fn allocator(&self) -> &A {
        &self.allocator
    }

    /// Leak this allocation. You should just take this as irreversible.
    pub fn leak(self) -> &'static T {
        // SAFETY: This is a unique pointer, so it won't conflict with any other
        //         references.
        unsafe { &*self.data.as_ptr() }
    }

    pub const fn as_ptr(&self) -> *mut T {
        self.data.as_ptr()
    }
}

impl<T, A> Unique<T, A>
where
    A: Allocator,
{
    /// Write to this pointer
    pub fn write(&mut self, value: T) {
        // SAFETY: We have ownership over this pointer (since it cannot be
        // safely copied or cloned).
        unsafe { ptr::write(self.data.as_ptr(), value) };
    }

    pub fn free(self) {
        // SAFETY:
        // - This is a unique pointer, so we can totall free it safely, by the
        //   same logic as [`std::boxed::Box<T>`]
        // - Of course, this is also non-null.
        unsafe {
            self.allocator
                .deallocate(self.data.cast(), Layout::new::<T>())
        }
    }
}

impl<T, A> Unique<MaybeUninit<T>, A>
where
    A: Allocator,
{
    pub fn init(mut self, value: T) -> Unique<T, A> {
        self.write(MaybeUninit::new(value));
        Unique {
            data: self.data.cast(),
            allocator: self.allocator,
        }
    }

    pub fn new_slice(allocator: A, count: usize) -> Unique<[MaybeUninit<T>], A> {
        Unique {
            data: NonNull::slice_from_raw_parts(
                allocator.xallocate(Layout::new_slice::<T>(count)).cast(),
                count,
            ),
            allocator,
        }
    }
}

impl<T, A> Unique<[T], A>
where
    A: Allocator,
{
    /// Construct a zero-sized slice. This pointer is dangling and doesn't
    /// actually do any allocating.
    pub const fn empty_slice(allocator: A) -> Self {
        Self {
            data: NonNull::slice_from_raw_parts(NonNull::<T>::dangling(), 0),
            allocator,
        }
    }

    pub fn free_slice(self) {
        // SAFETY: This should always be safe... I think. If I've not missed
        // anything, a unique pointer should basically always be safe to free.
        // And the move semantics of this pointer lets us do that.
        unsafe {
            self.allocator
                .deallocate(self.data.cast(), Layout::new_slice::<T>(self.data.len()))
        };
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        <Self as BorrowMut<[T]>>::borrow_mut(self)
    }

    pub fn as_slice(&self) -> &[T] {
        <Self as Borrow<[T]>>::borrow(self)
    }

    /// Same as getting the length of a fat pointer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the slice is empty
    pub fn is_empty(&self) -> bool {
        self.data.len() == 0
    }

    /// Attempt to grow this allocation, to hold at least `additional` more
    /// elements
    ///
    /// # Safety
    ///
    /// - `new_capacity` must be greater than or equal to the current capacity
    pub unsafe fn grow(&mut self, new_capacity: usize) {
        if new_capacity == 0 {
            // Do nothing! What a stupid caller.
            // This is technically an unsound operation -- let's catch that!
            debug_assert!(self.data.len() == 0);
            return;
        }
        let old_layout = Layout::new_slice::<T>(self.data.len());
        let new_layout = Layout::new_slice::<T>(new_capacity);
        // SAFETY:
        // - `ptr` is uniquely owned and allocated by `self.allocator()`
        // - `old_layout` is the exact layout we allocated with (so it fits)
        // - `new_layout` is greater than the current one (safety contract
        //   passed to caller)
        let alloc: NonNull<T> = if self.data.len() == 0 {
            let alloc: NonNull<T> = self.allocator().xallocate(new_layout).cast();
            // SAFETY: `new_layout` contains at least `new_capacity` elements!
            unsafe {
                alloc.as_ptr().write_bytes(0, new_capacity);
            }
            alloc
        } else {
            let alloc: NonNull<T> = unsafe {
                self.allocator()
                    .xgrow(self.data.cast(), old_layout, new_layout)
            }
            .cast();
            // We're just memset()ing the part that we just acquired
            // SAFETY: we know our new allocation to be at least the same size
            //         as the old allocation, so this is just a one-past-the-end
            //         pointer in the worst case!
            let maybe_non_zero_start = unsafe { alloc.as_ptr().add(self.data.len()) };
            // SAFETY: this is _at least_ the additional space we've got...
            unsafe {
                maybe_non_zero_start.write_bytes(0, new_capacity - self.data.len());
            }
            alloc
        };
        // SAFETY:
        // - we know this pointer must point to something with `new_layout`
        // - which holds _at least_ `new_capacity` elements!
        let data = ptr::slice_from_raw_parts_mut(alloc.as_ptr(), new_capacity);
        debug_assert!(non_null(data));
        // SAFETY:
        // - Must be non-null because we construct it from our alloc which is
        //   guaranteed to be non-null
        self.data = unsafe { NonNull::new_unchecked(data) };
    }
}

impl<T, A> Borrow<[T]> for Unique<[T], A>
where
    A: Allocator,
{
    fn borrow(&self) -> &[T] {
        // SAFETY: Same logic as `BorrowMut`
        unsafe { &*self.data.as_ptr() }
    }
}

impl<T, A> BorrowMut<[T]> for Unique<[T], A>
where
    A: Allocator,
{
    fn borrow_mut(&mut self) -> &mut [T] {
        // SAFETY: `Unique` is guaranteed to be the only pointer to this
        // allocation and this takes a `&mut` reference -- so tagged mutable
        unsafe { &mut *self.data.as_ptr() }
    }
}

pub trait AllocatorExt
where
    Self: Allocator,
{
    /// A version of `Allocator::allocate()` that panics on failure
    fn xallocate(&self, layout: Layout) -> NonNull<[u8]> {
        match self.allocate(layout) {
            Ok(data) => data,
            Err(e) => {
                panic!("Got an `AllocError` when trying to allocate(): {e:?}");
            }
        }
    }

    /// A version of `Allocator::grow()` that panics on failure
    ///
    /// # Safety
    ///
    /// Same safety contract as [`Allocator::grow`]
    unsafe fn xgrow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> NonNull<[u8]> {
        match unsafe { self.grow(ptr, old_layout, new_layout) } {
            Ok(data) => data,
            Err(e) => {
                panic!("Got an `AllocError` when trying to grow(): {e:?}");
            }
        }
    }
}

impl<A> AllocatorExt for A where A: Allocator {}
