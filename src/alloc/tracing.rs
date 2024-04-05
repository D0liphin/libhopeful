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

use crate::{
    alloc::manual::AllocatorExt,
    arch::mem::{memcpy_maybe_garbage, usize_load_acq},
    os::mem::StackAlloc,
    put, putln,
    util::num::round_up,
};
use std::{
    alloc::{Allocator, GlobalAlloc, Layout},
    cmp, fmt,
    mem::{self, transmute, MaybeUninit},
    ptr::{self, NonNull},
    sync::atomic::{AtomicPtr, AtomicUsize, Ordering},
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtrMeta {
    pub id: AllocId,
    pub offset: usize,
}

pub struct HexDump {
    buf: Vec<u8>,
}

impl HexDump {
    pub const fn new(buf: Vec<u8>) -> Self {
        Self { buf }
    }
}

impl fmt::Debug for HexDump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for i in 0..self.buf.len() {
            write!(f, "{:x}", self.buf[i])?;
            if i + 1 != self.buf.len() {
                write!(f, " ")?;
            }
        }
        write!(f, "]")
    }
}

pub enum FindPointerMethod {
    /// Find pointers that are aligned to `align_of::<usize>()` only, only
    /// searching on the heap
    AlignedHeapOnly,
}

impl AllocId {
    pub fn from_sized<T>(ptr: &T) -> Self {
        Self {
            ptr: ptr as *const T as *const (),
            size: mem::size_of::<T>(),
        }
    }

    /// Find all the pointers in an allocation, this returns a bunch of
    /// `PtrMeta` objects. The [`AllocId`] is the [`AllocId`] to which the
    /// pointer points, e.g. `vec![Box::new(5)]`, would return the [`AllocId`]
    /// of the [`Box`]. That means "where it points and how big the thing is
    /// that it points to".
    ///
    /// Each `PtrMeta` also has an `offset`, which is how many bytes from the
    /// start of the object this pointer is stored., e.g. that `Box(5)` from
    /// earlier, would just have `offset` 0.
    ///
    /// # Safety
    ///
    /// - This is unsafe. The thing is *why* it's unsafe is basically unknown,
    ///   so there's not really any point in me making it `unsafe` cos you can't
    ///   even verify that you're using it properly.
    pub fn find_pointers<A>(
        &self,
        global: &TracingAlloc<A>,
        method: FindPointerMethod,
    ) -> Vec<PtrMeta>
    where
        A: Allocator + FlatAllocator,
    {
        putln!("find_pointers()");
        let ghl = global.acquire_global_heap_lock();
        putln!("find_pointers(): acquired GHL");
        let alloc = StackAlloc::new(
            mem::size_of::<PtrMeta>() * (self.size / mem::size_of::<*const ()>())
                + mem::align_of::<PtrMeta>(),
        ); //   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
           //   This bit is absolutely unnecessary... uhh I really should remove
           //   it and prove the size is correct instead
        let alloc = &alloc;
        let mut buf: Box<[MaybeUninit<PtrMeta>], &StackAlloc> =
            Box::new_uninit_slice_in(self.size / mem::size_of::<*const ()>(), alloc);
        let mut i = 0usize;

        putln!("allocated Box<[]>");

        match method {
            FindPointerMethod::AlignedHeapOnly => {
                let p = {
                    let u8_ptr = self.ptr as *const u8;
                    let delta = round_up(self.ptr as usize, mem::align_of::<*const ()>())
                        - self.ptr as usize;
                    if delta >= self.size {
                        return Vec::new();
                    }
                    // SAFETY: This is guaranteed to be in the same allocation
                    // because of the above check
                    (unsafe { u8_ptr.add(delta) }) as *const usize
                };
                putln!("p start = ", p as usize);
                for p_offset in 0..buf.len() {
                    // SAFETY: buf.len() is floor(sizeof(alloc) / 4) so this
                    // should be exactly the number of pointer-aligned values in
                    // the allocation
                    let ptr_maybe_ptr = unsafe { p.add(p_offset) };
                    let maybe_ptr = unsafe { usize_load_acq(ptr_maybe_ptr) as *const () };
                    putln!("maybe_ptr = ", maybe_ptr as usize);
                    // SAFETY: we have the GHL
                    if let Some(id) = unsafe { global.find_unchecked(maybe_ptr) } {
                        putln!("twas a ptr");
                        let offset = ptr_maybe_ptr as usize - self.ptr as usize;
                        buf[i] = MaybeUninit::new(PtrMeta { id, offset });
                        i += 1;
                    }
                }
            }
        }

        drop(ghl);

        // SAFETY: Each case marks `i` as the next uninit element, with all
        // prior elements initialized.
        let init_buf: &[PtrMeta] = unsafe { transmute(&buf.as_ref()[0..i]) };
        Vec::from_iter(init_buf.iter().copied())
    }

    /// Read the bytes of this allocation, without stopping the world or any of
    /// that stuff
    ///
    /// # Safety
    ///
    /// - performs a [`memcpy_maybe_garbage()`] so follow the guide for that
    pub unsafe fn read_unchecked(&self) -> HexDump {
        let mut buf = vec![0u8; self.size];
        unsafe {
            memcpy_maybe_garbage(buf.as_mut_ptr(), self.ptr as *const u8, self.size);
        }
        HexDump::new(buf)
    }
}

pub struct AllocMeta {}

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

pub struct GlobalHeapLock<'a, A>
where
    A: Allocator,
{
    tracing_alloc: &'a TracingAlloc<A>,
}

impl<'a, A> GlobalHeapLock<'a, A>
where
    A: Allocator,
{
    pub fn new(tracing_alloc: &'a TracingAlloc<A>) -> Self {
        while !tracing_alloc.bit_map.try_acquire_writer_lock() {}
        // Normally, we also wait on readers, but we do not need to here, since
        // the GHL does not guarantee that memory will not be updated while we
        // hold the lock
        Self { tracing_alloc }
    }
}

impl<'a, A> Drop for GlobalHeapLock<'a, A>
where
    A: Allocator,
{
    fn drop(&mut self) {
        self.tracing_alloc.bit_map.release_writer_lock();
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
    nr_allocations: AtomicUsize,
}

impl<A> TracingAlloc<A>
where
    A: Allocator,
{
    /// Acquire the "global heap lock" (GHL). This locks the current heap from
    /// being modified, as viewed through the internal bitmap. The guarantees
    /// are not quite what you expect, namely:
    ///
    /// # Guarantees
    /// - All allocations at the time of acquisition remain active for the
    ///   duration that this lock is held. This means that the allocator
    ///   can't *unmap* a region that already exists -- it doesn't mean that
    ///   the bytes in that allocation are going to be "init" or valid for reads
    ///   of the type that you thought they were at the time of the lock
    ///   acquisition
    ///     - This includes `free()`
    ///
    /// # Non-guarantees
    /// - Memory in the current heap will stay the same for the duration you
    ///   hold the GHL
    /// - You can allocate (you can't -- use an arena instead)
    pub fn acquire_global_heap_lock(&self) -> GlobalHeapLock<A> {
        GlobalHeapLock::new(self)
    }
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
            nr_allocations: AtomicUsize::new(0),
        }
    }

    fn get_bit_map_index<T>(&self, ptr: *const T) -> Option<usize> {
        if (ptr as usize) < self.allocator().min_vaddr() {
            return None;
        }
        let bmi = ptr as usize - self.allocator().min_vaddr();
        Some(bmi)
    }

    unsafe fn alloc_id_from_ptr_steps<T>(
        &self,
        ptr: *const T,
        steps: Option<usize>,
    ) -> Option<AllocId> {
        match steps {
            Some(steps) => {
                putln!("found in ", steps, " steps");
                // TODO: verify that this matches _every_ case
                let alloc_ptr = (ptr as *const u8).wrapping_offset(-(steps as isize));
                let header_ptr = alloc_ptr as usize - mem::size_of::<TracingAllocHeader>();
                putln!("find(): header_ptr = ", header_ptr);
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

    /// # Safety
    ///
    /// - `find()` without locking first -- you should ensure that you have a
    ///   lock (either reader or writer) yourself.
    pub unsafe fn find_unchecked<T>(&self, ptr: *const T) -> Option<AllocId> {
        let bit_map_index = self.get_bit_map_index(ptr)?;
        // SAFETY: requirement passed to caller
        let bit = unsafe { self.bit_map.get_unsync(bit_map_index) };
        let steps = bit.scan_backward_no_drop();
        unsafe { self.alloc_id_from_ptr_steps(ptr, steps) }
    }

    /// TODO: Docs
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn find<T>(&self, ptr: *const T) -> Option<AllocId> {
        let bit_map_index = self.get_bit_map_index(ptr)?;
        putln!("find(", ptr as usize, " -> ", bit_map_index, ")");
        let steps = self.bit_map.get(bit_map_index).scan_backward();
        unsafe { self.alloc_id_from_ptr_steps(ptr, steps) }
    }

    /// TODO: Docs
    pub fn nr_allocations(&self) -> usize {
        self.nr_allocations.load(Ordering::Acquire)
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

        // SAFETY: The allocation is at least as big as
        // mem::size_of::<TracingAllocHeader>()
        let header_ptr = unsafe {
            data.cast::<u8>()
                .add(header_size - mem::size_of::<TracingAllocHeader>())
        };

        // SAFETY: This is always going to be aligned for dlmalloc, but we don't
        //         know that this is dlmalloc!
        unsafe {
            header_ptr
                .cast::<TracingAllocHeader>()
                .write_unaligned(header)
        }

        // SAFETY: we know this to be within the bounds of the allocation (or
        //         one past the end).
        let mut data = unsafe { data.cast::<u8>().add(header_size) };
        debug_assert!(data.is_aligned_to(layout.align()));

        put!("header_ptr = ", header_ptr.as_ptr() as usize, "|");
        for i in 0..16 {
            put!(
                unsafe { header_ptr.cast::<u8>().add(i) }.as_ptr().read() as usize,
                " "
            );
        }
        putln!("");

        let index = self
            .get_bit_map_index(data.as_mut())
            .expect("Any valid allocation should be greater than min_vaddr");
        putln!(
            "self.bit_map.set_high(",
            data.as_ptr() as usize,
            " -> ",
            index,
            ")"
        );
        self.bit_map.set_high(index);

        self.nr_allocations.fetch_add(1, Ordering::Release);
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
        self.nr_allocations.fetch_sub(1, Ordering::Release);
    }
}

/*
alloc(size: 21, align: 1, header_size: 16)
header_ptr = 106066436763664|5 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
ensure_index_exists(): 0 is not in bounds!
ensure_index_exists(): dec_nr_readers()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
ensure_index_exists(): 0 is still not in bounds!
grow(0)
grow(): try_acquire_writer_lock()
grow(): got writer_lock!
grow(): wait_on_readers()
wait_on_readers(): nr_readers = 0
wait_on_readers(): nr_readers = 0, done waiting!
grow(): buf.grow(16)
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 64, align: 8, header_size: 16)
header_ptr = 106066436763840|48 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436763920|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 176, align: 8, header_size: 16)
header_ptr = 106066436763952|160 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764144|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764176|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764208|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764240|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764272|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764304|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764336|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764368|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764400|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764432|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764464|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764496|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764528|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764560|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764592|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764624|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436764656|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
ensure_index_exists(): 16 is not in bounds!
ensure_index_exists(): dec_nr_readers()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
ensure_index_exists(): 16 is still not in bounds!
grow(16)
grow(): try_acquire_writer_lock()
grow(): got writer_lock!
grow(): wait_on_readers()
wait_on_readers(): nr_readers = 0
wait_on_readers(): nr_readers = 0, done waiting!
grow(): buf.grow(32)
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436763696|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
alloc(size: 20, align: 4, header_size: 16)
header_ptr = 106066436763728|4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
ensure_index_exists()
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
inc_nr_readers()
inc_nr_readers(): finished waiting on writer_lock
inc_nr_readers(): finished
self.chunk_index, self.bit_index = 6, 32
current chunk (6), unmasked, masked = 0000000000000000000000000000000000000000000000000000000000000000,
        0000000000000000000000000000000000000000000000000000000000000000
chunk_index = 5, chunk = 1000000000000000000000000000000000000000000000000000000000000000
trailing_zeros() = 0
dec_nr_readers()
dec_nr_readers(): start: nr_readers = 1
dec_nr_readers(): end: nr_readers = 0
found in 32 steps
find(): header_ptr = 106066436764016
[examples/tests.rs:18:5] alloc_id = AllocId {
    ptr: 0x0000607784493180,
    size: 106066436764352,
}
find_pointers()
find_pointers(): acquired GHL
StackAlloc::new(2545594482344456)
alloc(size: 44, align: 1, header_size: 16)
header_ptr = 106066436763760|28 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
inc_nr_readers()
*/
