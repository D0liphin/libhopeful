// /*
// The main challenge here is to get the bitmap to correspond correctly with the
// actual heap state... We don't have to guarantee any order of operations, with
// set_high() and set_low(), because they're not actually based on the current
// state of the bitmap. We would just allocate every time we don't have enough
// space (which would probably be a little wasteful -- if this is better than
// locking is debatable).

// +-----------------+-----------------+
// |    THREAD 1     |    THREAD 2     |
// |-----------------+-----------------|
// | set_high(5)     |                 |
// |                 | set_low(5)      |
// | realloc()       | realloc()       |
// |                 | mask_with_new() |
// | mask_with_new() |                 |
// +-----------------+-----------------+

// THREAD 1 and THREAD 2 don't know what the other thread is doing. In this case,
// even if the set_low(5) instructino from THREAD 2 was issued "after" THREAD 1,
// we don't care. The whole thing can actually be Ordering::Relaxed... though that
// might not be the most lovely thing in the world.

// Now, the issue is that we want to make sure these operations are bundled
// atomically with our calls to `dlmalloc()`. If `dlmalloc()` allocates something
// and then frees it on another thread, we must not mark that region as allocated!
// It is technically possible for somebody to poll the heapstate inbetween an
// free() and an update to the bitmask. That's actually fine. We do byte-wise reads
// in assembly. It would just produce some garbage. The issue is this case:

// +-----------------+-----------------+
// |    THREAD 1     |    THREAD 2     |
// |-----------------+-----------------|
// | alloc(n)        |                 |
// | register(n)     |                 |
// |                 | free(n)         |
// |                 | alloc(n)        |
// |                 | deregister(n)   |
// |                 | register(n)     |
// +-----------------+-----------------+

// Here, we believe something is freed when it's not. This means that we can't
// trace down that path when we should be able to. If that live object points to
// something else that is live, we could mark it for sweeping... Oh no!

// To prevent this, we just only expose safe interfaces to `free() -> deregister()`
// and `alloc() -> register()` **sequentially on the same thread**. We provide no
// safe way of allocating without registering etc.

// Since we provide no way of synchronizing based on the malloc() of free() calls
// (those are implementation details of the block of synchronous code), we can
// happily assert that these "deregister without first registering orderings"
// happen. Wow, that was a really long way of saying that.

// # Some more Notes 11-03-2024

// We want local writes to be *guaranteed ordered*, but writes from other threads
// can interfere whenever we want (unless the user explictly synchronizes). But
// this means also synchronized on some global variable... That's the tricky part
// to get right

// The hard part is making sure that we don't lose any writes if *somebody* does a
// set_high() we can't just forget about it. That's a logic error. The way we can
// get around this is with "double-check copying".

// [0, 0]

// Let's say I want to set the bit at index [2] to 1. First we realloc a new buffer
// [?, ?, ?]. Reading from this buffer would be a logic error -- it could have any
// value. Then we go through each value in the old buffer and copy it over, in
// order.

// data : [0, 0]
// swap : [0, ?, ?]
// use  : swap[..0] data[0..]

// This is one atomic operation. After this, some other thread might update data,
// because we have not marked the swap as having an init value for the range [..1].

// data : [1, 0]
// swap : [0, ?, ?]
// use  : swap[..1] data[1..]

// Ok so we can solve a lot of issues when we just consider that we really don't
// need to synchronize anything... The reallocating thread is easy to prove as well
// the question is about another thread. On the reallocating thread we do these
// atomic operations:

// ```
// copy(data)
// store(data, swap)
// mark_stored(swap)
// ```

// On the other thread, we do these atomic operations:

// ```
// store(0, bitmap)
// store(1, bitmap)
// store(0, bitmap)
// ```

// This *must* end up as 0 in all reorderings, and must also have a visible 1 on
// the local thread immediately after we store it. It is very obvious how this is
// not possible with the naive approach. We can take advantage of the fact that
// this is very unlikely to actually happen and instead change our reallocating
// code to

// ```
// 'start:
//     copy(data)
//     store(data, swap)
//     if swap == data then mark_stored(swap) else goto 'start
// ```

// You might think it possible to do something like

// ```
// copy(data)
// store(data, swap)
// mark_stored(swap)
// copy(data)
// store(data, swap)
// ```

// Here is proof of unsoundness by counter example:

// ```
// A: store(0, bitmap)
// R: copy(data)
// A: store(1, bitmap)
// R: store(data, swap)
// R: mark_stored(swap)
//     <- reads by A here read 0, not 1
// A: store(0, bitmap)
// R: copy(data)
// R: store(data, swap)
//     <- reads by A here read 1, not 0
// ```

// copy the data over, then we can do a compare to see how data, swap and tmp
// change!
// */
// fn bit_at(n: usize, index: usize) -> bool {
//     n >> index & 1 == 1
// }

// use std::{
//     alloc::Allocator,
//     cmp,
//     mem::{self, MaybeUninit},
//     ptr,
//     sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering},
// };

// use bytemuck::checked::cast;
// use crossbeam::epoch::Atomic;

// use crate::alloc::manual::Unique;

// type AtomicBitChunk = AtomicU64;

// pub struct AtomicBufHandle<'a> {
//     /// For the update to actually be atomic, we're validating that this
//     /// pointer is OK.
//     pub cell: &'a AtomicBitChunk,
//     /// The 'position' of this chunk. We do a `cmp_exchange` on this value to
//     /// make sure we can actually update it.
//     pub index: usize,
//     /// The offset into this chunk that we will perform our bit update.
//     pub offset: usize,
// }

// /// This data structure is specifically for searching for a high-bit. It's not
// /// really supposed to be an all-purpose bitvec. Also, it's *mostly* lock-free
// /// but not entirely lock-free. If you try and realloc twice in a row, you will
// /// get locked. We can make this lock-free, but locking is probably a better
// /// solution here I think?
// pub struct BitMap {
//     /// This is already 8-byte aligned... so we can use an `AtomicU8`, but we
//     /// use `u64` so we can do faster operations.
//     data: AtomicPtr<AtomicU64>,
//     /// The bitmap can only *grow*, so this is actually a lowerbound length...
//     len: AtomicUsize,
//     /// We use this during reallocations, as described in the explanation at the
//     /// top of this file
//     swap: AtomicPtr<AtomicU64>,
//     swap_len: AtomicUsize,
// }

// impl BitMap {
//     pub const fn new() -> Self {
//         Self {
//             data: AtomicPtr::new(ptr::null_mut()),
//             len: AtomicUsize::new(0),
//             swap: AtomicPtr::new(ptr::null_mut()),
//             swap_len: AtomicUsize::new(0),
//         }
//     }

//     pub fn data_acquire(&self) -> *mut AtomicU64 {
//         self.data.load(Ordering::Acquire)
//     }

//     pub fn len_acquire(&self) -> usize {
//         self.len.load(Ordering::Acquire)
//     }

//     pub fn swap_acquire(&self) -> *mut AtomicU64 {
//         self.swap.load(Ordering::Acquire)
//     }

//     pub fn swap_len_acquire(&self) -> usize {
//         self.swap_len.load(Ordering::Acquire)
//     }

//     /// Get the index and bit offset for a specified index. For example, if we
//     /// are using 64-bit chunks, then index 74 would return (1, 10). The second
//     /// number is guaranteed to be no greater than `size_of::<AtomicBitChunk>()`
//     ///
//     /// The chunk index is not guaranteed to be accessible. You should check
//     /// first.
//     fn index_offset(index: usize) -> (usize, usize) {
//         let chunk_index = index / mem::size_of::<AtomicU64>();
//         let bit_offset = index % chunk_index;
//         (chunk_index, bit_offset)
//     }

//     /// Test if this [`BitMap`] is currently in the middle of a reallocation --
//     /// in this case, we might have to lock. The hope is this almost never
//     /// happens.
//     fn is_mid_realloc(&self) -> bool {
//         self.swap_len_acquire() != 0
//     }

//     /// Get the chunk from the appropriate buffer to apply an update to
//     ///
//     /// # Safety
//     ///
//     /// - `index` must be less than `max(self.swap_len, self.len)`
//     pub unsafe fn get_unchecked_acquire(&self, index: usize) -> AtomicBufHandle {
//         debug_assert!(index < cmp::max(self.len_acquire(), self.swap_len_acquire()));
//         let (index, offset) = Self::index_offset(index);
//         let chunk = if self.is_mid_realloc() {
//             // SAFETY: both pointers are guaranteed to point to buffers at least
//             //         the size of their associated `len`. This assertion is
//             //         passed to the caller.
//             unsafe { &*self.swap_acquire().add(index) }
//         } else {
//             // SAFETY: as above
//             unsafe { &*self.data_acquire().add(index) }
//         };
//         AtomicBufHandle {
//             cell: chunk,
//             index,
//             offset,
//         }
//     }

//     /// Attempt to set the bit at `index` to `1`.
//     ///
//     /// # Safety
//     ///
//     /// - `assert!(index < max!(self.len, self.swap_len))`. The index
//     ///   that we are inserting at must be less than the maximum of `self.len`
//     ///   and `self.swap_len`.
//     pub unsafe fn set_high_unchecked(&self, index: usize) {
//         debug_assert!(index < cmp::max(self.len_acquire(), self.swap_len_acquire()));
//         let (index, offset) = Self::index_offset(index);
//         let chunk = if self.is_mid_realloc() {
//             // SAFETY: both pointers are guaranteed to point to buffers at least
//             //         the size of their associated `len`. This assertion is
//             //         passed to the caller.
//             unsafe { &*self.swap_acquire().add(index) }
//         } else {
//             // SAFETY: as above
//             unsafe { &*self.data_acquire().add(index) }
//         };
//         chunk.fetch_or(1u64 << offset, Ordering::Release);
//     }

//     // pub fn realloc<A>(&self, additional: usize, allocator: A) -> Self
//     // where
//     //     A: Allocator,
//     // {
//     //     let min_len = self.len_acquire() + additional;
//     //     let len = 1 << (usize::BITS << min_len.leading_zeros());
//     //     self.swap.store(
//     //         Unique::<MaybeUninit<AtomicU64>, A>::new_slice(allocator, len)
//     //             .as_ptr()
//     //             .cast(),
//     //         Ordering::Release,
//     //     );
//     //     // We obviously don't want to update `swap` or `data` mid realloc()
//     //     let swap = self.swap.load(Ordering::Acquire);
//     //     let data = self.data.load(Ordering::Acquire);
//     //     let len = self.len_acquire();
//     //     let mut i = 0;
//     //     loop {
//     //         if i == len {
//     //             break;
//     //         }
//     //         let data_el = unsafe { &*data.add(i) };
//     //         let swap_el = unsafe { &*swap.add(i) };
//     //         // TODO: double check if Ordering::Relaxed is actually okay here
//     //         swap_el.store(data_el.load(Ordering::Acquire), Ordering::Relaxed);
//     //         while swap_el.load(Ordering::Acquire) != data_el.load(Ordering::Acquire) {}
//     //         i += 1;
//     //     }
//     //     todo!()
//     // }
// }

// // impl<A> BitMap<A>
// // where
// //     A: Allocator + Copy + Default,
// // {
// // /// Constructs a new, empty bitmask
// // pub const fn new(allocator: A) -> Self {
// //     Self {
// //         chunks: AtomicCell::new(Unique::empty_slice(allocator)),
// //     }
// // }

// // fn allocator(&self) -> A {
// //     A::default()
// // }

// // /// Get the index and bit offset for a specified index. For example, if we
// // /// are using 64-bit chunks, then index 74 would return (1, 10). The second
// // /// number is guaranteed to be no greater than `size_of::<AtomicBitChunk>()`
// // ///
// // /// The chunk index is not guaranteed to be accessible. You should check
// // /// first.
// // fn index_offset(index: usize) -> (usize, usize) {
// //     let chunk_index = index / size_of::<AtomicBitChunk>();
// //     let bit_offset = index % chunk_index;
// //     (chunk_index, bit_offset)
// // }

// // pub fn realloc(&self, additional: usize) {
// //     if additional == 0 {
// //         return;
// //     }
// //     let min_capacity = self.chunks.len() + additional;
// //     // If we have 15 (<< 16) leading zeroes, we want a new capacity with 14
// //     // (<< 17) leading zeroes. 32 - 15 = 17!
// //     let capacity = 1 << ((size_of::<usize>() * 8) << min_capacity.leading_zeros());
// //     let mut new_chunks: Unique<[MaybeUninit<AtomicBitChunk>], A> =
// //         Unique::new_slice(self.allocator(), capacity);
// //     // for chunk in new_chunks.as_mut_slice() {

// //     // }
// // }

// // /// Set a bit high, reallocate to the size of `index`
// // pub fn set_high(&self, index: usize) {
// //     let (chunk_index, bit_offset) = Self::index_offset(index);
// //     if !(0..self.chunks.len()).contains(&chunk_index) {
// //         // self.realloc()
// //     }
// //     todo!()
// // }

// // pub fn set_low(&self, index: usize) {
// //     todo!()
// // }
// // }

// /*
// We are aiming for a lock-free usage from the POV of the allocator -- we don't
// want the user to be locked on some kind of update to this structure. It should
// have relatively predicatable performance. Our BitMap has an exponential
// regrowth strategy, so it is very unlikely that the user needs to realloc twice,
// within a very short timespan (so long as we allocate a sufficiently large
// initial buffer).

// The idea is to have two buffers: `data` and `swap`. Updates to a location in
// either buffer requires acquiring a handle to that buffer. So, if I want to
// update data, I get an `AtomicBufHandle<'a, AtomicU64>`. After that handle is
// dropped, we decrement the counter.

// When we do `get_acquire()`, we are fishing for a chunk in the buffer that is
// currently being used. There are three possibilities:

// ```
// ┌───┬───┬───┬───┬───┬───┬───┐
// │   │   │   │   │   │   │   │
// └───┴───┴───┴───┴───┴───┴───┘
// ┌───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┐
// │swp│swp│swp│   │   │   │   │   │   │   │   │   │   │   │
// └───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┘
// ```

// Here, we provide pointers to the swapped section of the `swap`, the unswapped
// section of `data`, or the dangling section of `swap` depending on where index is
// in this case:

// ```
// 0..3  -> swap
// 3..7  -> data
// 7..14 -> swap
// ```

// For each potential update, we acquire a handle. Before swapping over a chunk
// from `data` to `swap`, we `futex()` on the count of handles to `data`.
// */
// /*
// Nah, that's cap. What we're going to do is have a handle. The handle represents
// a pointer into a buffer and a 'location' for that pointer. Something like

// ```
// struct AtomicBufHandle<'a> {

// }
// ```

// Imagine we are always in the middle of an allocation: in this case we just
// compare exchange on the length of
// */
use std::{
    cell::UnsafeCell,
    cmp, fmt, mem,
    sync::atomic::{fence, AtomicU64, AtomicUsize, Ordering},
};

use linux_futex::{Futex, Private};

use crate::{thread_println, util::num::log2ceil};

use super::{dlmalloc::DlMalloc, manual::Unique};

pub type AtomicBitChunk = AtomicU64;
pub type BitChunk = u64;

/// Get the index and bit offset for a specified index. For example, if we
/// are using 64-bit chunks, then index 74 would return (1, 10). The second
/// number is guaranteed to be no greater than `size_of::<AtomicBitChunk>()`
///
/// The chunk index is not guaranteed to be accessible. You should check
/// first.
fn index_offset(index: usize) -> (usize, usize) {
    let chunk_index = index / BitChunk::BITS as usize;
    let bit_offset = index % BitChunk::BITS as usize;
    (chunk_index, bit_offset)
}

pub struct ReadGuard<'a> {
    bit_map: &'a BitMap,
}

impl<'a> ReadGuard<'a> {
    fn new(bit_map: &'a BitMap) -> Self {
        bit_map.inc_nr_readers();
        Self { bit_map }
    }
}

impl<'a> Drop for ReadGuard<'a> {
    fn drop(&mut self) {
        thread_println!("ReadGuard::drop()");
        self.bit_map.dec_nr_readers();
    }
}

pub struct BitHandle<'a> {
    read_guard: ReadGuard<'a>,
    chunk_index: usize,
    bit_index: usize,
}

impl<'a> BitHandle<'a> {
    fn new(bit_map: &'a BitMap, index: usize) -> Self {
        let (chunk_index, bit_index) = index_offset(index);
        Self {
            read_guard: ReadGuard::new(bit_map),
            chunk_index,
            bit_index,
        }
    }

    fn bit_map(&self) -> &BitMap {
        self.read_guard.bit_map
    }

    fn buf(&self) -> &Unique<[AtomicBitChunk], DlMalloc> {
        // TODO: Safety
        unsafe { &*self.read_guard.bit_map.buf.get() }
    }

    fn ensure_index_exists(&self) {
        thread_println!("ensure_index_exists()");
        if !self.bit_map().chunk_in_bounds(self.chunk_index) {
            thread_println!(
                "ensure_index_exists(): {} is not in bounds!",
                self.chunk_index
            );
            thread_println!("ensure_index_exists(): dec_nr_readers()");
            self.bit_map().dec_nr_readers();
            while !self.bit_map().chunk_in_bounds(self.chunk_index) {
                thread_println!(
                    "ensure_index_exists(): {} is still not in bounds!",
                    self.chunk_index
                );
                self.bit_map().grow(self.chunk_index);
            }
        }
        self.bit_map().inc_nr_readers();
    }

    pub fn set_high(self) {
        self.ensure_index_exists();
        // Now we know that the index exists!
        self.buf().as_slice()[self.chunk_index].fetch_or(1 << self.bit_index, Ordering::Release);
    }
}

pub struct BitMap {
    buf: UnsafeCell<Unique<[AtomicBitChunk], DlMalloc>>,
    /// The length itself is obviously stored twice -- but this is actually
    /// quite convenient and not an issue, since we only have one `BitMap` for
    /// the whole program!
    buf_len: AtomicUsize,
    writer_lock: Futex<Private>,
    /// The number of handles to the buffer. We acquire the lock, and then we
    /// have to wait for all the handles to get consumed
    nr_readers: Futex<Private>,
}

impl fmt::Debug for BitMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        _ = self.acquire_read_guard();
        for chunk in unsafe { &*self.buf.get() }.as_slice() {
            let chunk = chunk.load(Ordering::Acquire);
            write!(f, "{chunk:b} ")?;
        }
        Ok(())
    }
}

// TODO: Safety comment
unsafe impl Sync for BitMap {}

const FUTEX_UNLOCKED: u32 = 0;
const FUTEX_LOCKED: u32 = 1;

impl BitMap {
    /// # Safety
    ///
    /// `DlMalloc` must be registered as the global allocator *or* a `DlMalloc`
    /// derivative (e.g. `DlMallocMeta`)
    pub const unsafe fn new() -> Self {
        Self {
            buf: UnsafeCell::new(Unique::empty_slice(DlMalloc::new())),
            buf_len: AtomicUsize::new(0),
            writer_lock: Futex::new(FUTEX_UNLOCKED),
            nr_readers: Futex::new(0),
        }
    }

    /// Acquires a reader lock, while this is live, you know that you can do any
    /// 'read' operation you like. That is defined as anything that takes &self,
    /// so not really 'reads', but atomic writes too. This essentially just
    /// doest `inc_nr_readers()`
    /// reading.
    pub fn acquire_read_guard(&self) -> ReadGuard {
        thread_println!("acquire_read_guard()");
        ReadGuard::new(self)
    }

    /// Acquire a reader lock that you have to manually release -- use
    /// `acquire_read_guard()` for a `Drop` guard instead.
    fn inc_nr_readers(&self) {
        thread_println!("inc_nr_readers()");
        loop {
            // Fast path should stay in userspace!
            _ = self.writer_lock.wait(FUTEX_LOCKED);
            thread_println!("inc_nr_readers(): finished waiting on writer_lock");
            // <-- [1]
            self.nr_readers.value.fetch_add(1, Ordering::Release);
            if self.writer_lock.value.load(Ordering::Acquire) == FUTEX_LOCKED {
                // A new writer might have acquired the lock at [1], in which
                // case, we release our reader lock and wait on it.
                self.nr_readers.value.fetch_sub(1, Ordering::Release);
                continue;
            }
            break;
        }
        thread_println!("inc_nr_readers(): finished");
    }

    /// `dec_nr_readers()` is used to release a reader lock, and is carried out
    /// as part of the drop method for `ReadGuard`. Conceptually we do the
    /// following atomic steps
    ///
    /// 1. Decrement the number of readers
    /// 2. If there are no more readers, wake the writer who is waiting on
    ///    readers. All other writers should be waiting on the `writer_lock`
    fn dec_nr_readers(&self) {
        thread_println!("dec_nr_readers()");
        thread_println!(
            "dec_nr_readers(): start: nr_readers = {}",
            self.nr_readers.value.load(Ordering::Acquire)
        );
        if self.nr_readers.value.fetch_sub(1, Ordering::Release) == 1 {
            // We could achieve a correct structure, just by waking everything
            // every time, but we do less syscalls if we only wake the writer
            // up on the final decrement...
            //
            // WE SHOULD ONLY HAVE **ONE** WRITER WAITING ON THIS. This should
            // also only happen if `write_lock` has been acquired, so we know
            // that this cannot increment again (and thus we can assert that
            // the woken writer genuinely does not have any contending readers).
            self.nr_readers.wake(1);
        }
        thread_println!(
            "dec_nr_readers(): end: nr_readers = {}",
            self.nr_readers.value.load(Ordering::Acquire)
        );
    }

    /// Wait for all readers to finish whatever they're doing. You **must** have
    /// the `writer_lock` for this method to make sense.
    fn wait_on_readers(&self) {
        thread_println!("wait_on_readers()");
        // We know that `nr_readers` cannot increase again here, since the
        // caller has asserted they have the `writer_lock`, so we don't have to
        // worry about `inc_nr_readers()` inserting an evil instruction here
        loop {
            let nr_readers = self.nr_readers.value.load(Ordering::Acquire);
            thread_println!("wait_on_readers(): nr_readers = {nr_readers}");
            if nr_readers == 0 {
                // if nr_readers is already 0, we can safely break (it is capped
                // at 0)
                thread_println!("wait_on_readers(): nr_readers = 0, done waiting!");
                break;
            }
            // Wait until we update nr_readers to something other than the
            // current value -- if the update was so fast that we never had any
            // futex contention, keep busywaiting, otherwise go to sleep
            thread_println!("wait_on_readers(): nr_readers.wait_for_update()");
            _ = self.nr_readers.wait(nr_readers);
        }
    }

    /// Acquire a [`BitHandle`] to the bit at bit-index `index`. Note that you
    /// **must** drop this as soon as you can to reduce lock contention.
    ///
    /// # Stuck
    ///
    /// - While the [`BitHandle`] exists!
    pub fn get(&self, index: usize) -> BitHandle {
        thread_println!("get({index})");
        BitHandle::new(self, index)
    }

    pub fn set_high(&self, index: usize) {
        thread_println!("set_high({index})");
        self.get(index).set_high();
    }

    /// Attempt to lock this `BitMap` for writer access. If another writer holds
    /// the lock already, we wait until they are done with it. It is **not
    /// sound** to assume you have the lock unless `true` is returned. You must
    /// keep retrying until this function returns `true`. Attempting to acquire
    /// the lock after a successful acquisition will result in deadlock.
    fn try_acquire_writer_lock(&self) -> bool {
        thread_println!("try_acquire_writer_lock()");
        // Try and acquire the lock, if successful, break;
        if let Ok(FUTEX_UNLOCKED) = self.writer_lock.value.compare_exchange(
            FUTEX_UNLOCKED,
            FUTEX_LOCKED,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            fence(Ordering::Acquire);
            thread_println!("try_acquire_writer_lock(): acquired lock");
            true
        } else {
            // Wait, if the lock is acquired by another thread
            _ = self.writer_lock.wait(FUTEX_LOCKED);
            false
        }
    }

    fn release_writer_lock(&self) {
        fence(Ordering::Release);
        self.writer_lock
            .value
            .store(FUTEX_UNLOCKED, Ordering::Release);
    }

    /// Check if the chunk_index at the specified index is within bounds
    pub fn chunk_in_bounds(&self, chunk_index: usize) -> bool {
        self.buf_len.load(Ordering::Acquire) > chunk_index
    }

    /// Double the size of this
    pub fn grow(&self, to_fit_chunk_at: usize) {
        thread_println!("grow({to_fit_chunk_at})");
        let chunk_index = to_fit_chunk_at;
        loop {
            // We can only grow, so we know that this is definitely true
            if self.chunk_in_bounds(chunk_index) {
                thread_println!("grow(): {chunk_index} already in bounds -- returning!");
                return;
            }
            thread_println!("grow(): try_acquire_writer_lock()");
            if self.try_acquire_writer_lock() {
                thread_println!("grow(): got writer_lock!");
                break;
            }
        }
        thread_println!("grow(): wait_on_readers()");
        self.wait_on_readers();

        // SAFETY: We have exclusive access *and* no other references can exist
        // since we wait for all readers to finish.
        let buf = unsafe { &mut *self.buf.get() };
        let sz: usize = cmp::max(log2ceil(chunk_index as _) as _, 16);
        thread_println!("grow(): buf.grow({sz})");
        // SAFETY:
        // - Checked if the `chunk_index` is in bounds
        // - Since it is not, it must be greater than the current capacity
        unsafe {
            buf.grow(sz);
        }
        // This is fine, because of the memory barrier later -- probably the
        // optimiser figured that out anyway
        self.buf_len.store(buf.len(), Ordering::Relaxed);

        self.release_writer_lock();
    }
}
