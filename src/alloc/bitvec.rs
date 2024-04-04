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
    alloc::Allocator,
    cell::UnsafeCell,
    cmp,
    ffi::CStr,
    fmt,
    io::{Cursor, Write},
    mem,
    sync::atomic::{fence, AtomicU64, AtomicUsize, Ordering},
};

use libc::{c_char, fputs, putchar, puts};
use linux_futex::{Futex, Private};

use crate::{
    put, putln, thread_println,
    util::{
        num::log2ceil,
        print::{endl, putstr, write_usize, ArrayIter, AsciiRepr, Bits, MAX_NR_CHARS_USIZE},
    },
};

use super::manual::Unique;

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

/// RAII read guard. If one of these exists, you have full read access (i.e. you
/// can get a `&Unique<AtomicBitChunk>` from the `BitMap`). `Drop` impl releases
/// the lock
pub struct ReadGuard<'a, A>
where
    A: Allocator,
{
    bit_map: &'a BitMap<A>,
}

impl<'a, A> ReadGuard<'a, A>
where
    A: Allocator,
{
    /// Construct a new `ReadGuard`, doing all the correct synchronisation stuff
    /// maybe block!
    fn new(bit_map: &'a BitMap<A>) -> Self {
        // put!("ReadGuard::new() ", *bit_map, endl());
        bit_map.inc_nr_readers();
        Self { bit_map }
    }
}

impl<'a, A> Drop for ReadGuard<'a, A>
where
    A: Allocator,
{
    fn drop(&mut self) {
        // putln!("ReadGuard::drop() ", *self.bit_map);
        self.bit_map.dec_nr_readers();
    }
}

/// Store the `chunk_index` and `bit_index` components of a bit index. Also
/// contains a guard, so `&` operations will always be safe!
pub struct BitHandle<'a, A>
where
    A: Allocator,
{
    read_guard: ReadGuard<'a, A>,
    chunk_index: usize,
    bit_index: usize,
}

impl<'a, A> BitHandle<'a, A>
where
    A: Allocator,
{
    /// Construct a new [`BitHandle`], acquiring a guard in the process
    fn new(bit_map: &'a BitMap<A>, index: usize) -> Self {
        let (chunk_index, bit_index) = index_offset(index);
        Self {
            read_guard: ReadGuard::new(bit_map),
            chunk_index,
            bit_index,
        }
    }

    /// Get the internal [`BitMap`]
    fn bit_map(&self) -> &BitMap<A> {
        self.read_guard.bit_map
    }

    /// Get the internal buf pointer for reads
    fn buf(&self) -> &Unique<[AtomicBitChunk], A> {
        // TODO: Safety
        unsafe { &*self.read_guard.bit_map.buf.get() }
    }

    /// Check that we can actually do stuff with this bit-handle. If we can't,
    /// *temporarily* release this lock and realloc the [`BitMap`]'s buffer to
    /// fit.
    fn ensure_index_exists(&self) {
        putln!("ensure_index_exists()");
        if !self.bit_map().chunk_in_bounds(self.chunk_index) {
            putln!(
                "ensure_index_exists(): ",
                self.chunk_index,
                " is not in bounds!",
            );
            putln!("ensure_index_exists(): dec_nr_readers()");
            self.bit_map().dec_nr_readers();
            while !self.bit_map().chunk_in_bounds(self.chunk_index) {
                putln!(
                    "ensure_index_exists(): ",
                    self.chunk_index,
                    " is still not in bounds!",
                );
                self.bit_map().grow(self.chunk_index);
            }
            self.bit_map().inc_nr_readers();
        }
    }

    /// Set the bit at this index to `1`
    pub fn set_high(self) {
        self.ensure_index_exists();
        // Now we know that the index exists!
        self.buf().as_slice()[self.chunk_index].fetch_or(1 << self.bit_index, Ordering::Release);
    }

    /// Set the bit at this index to `0`
    pub fn set_low(self) {
        self.ensure_index_exists();
        // Now we know that the index exists!
        self.buf().as_slice()[self.chunk_index]
            .fetch_and(!(1 << self.bit_index), Ordering::Release);
    }

    /// Scan backward, starting at the current index, looking for a high bit.
    /// Returns the number of steps taken, or `None` if no high bit exists.
    pub fn scan_backward(self) -> Option<usize> {
        if !self.bit_map().chunk_in_bounds(self.chunk_index) {
            return None;
        }

        // We're storing in the order msb <- lsb. And we're walking *backwards*
        // so we knock off all the stuff that is after the current index.
        let current_unmasked = self.buf().as_slice()[self.chunk_index].load(Ordering::Acquire);
        let current = current_unmasked << (BitChunk::BITS as usize - 1 - self.bit_index);
        putln!(
            "self.chunk_index, self.bit_index = ",
            self.chunk_index,
            ", ",
            self.bit_index
        );
        putln!(
            "current chunk (",
            self.chunk_index,
            "), unmasked, masked = ",
            Bits::<64>::new(current_unmasked),
            ", \n\t",
            Bits::<64>::new(current),
        );

        let mut steps = if current == 0 {
            // Nothing in this chunk, so we step backwards one extra (to get to
            // the next chunk)
            self.bit_index
        } else {
            // We mask self.bit_index bits, so let's do the example with 8 again
            // 0b_????_???? mask 5
            // 0b_???0_0000
            // [5] -> 5 - 5 = 0 (correct!)
            // [7] -> 7 - 5 = 2 (5, 6 correct!)
            return Some(current.leading_zeros() as usize);
        };

        for chunk_index in (0..self.chunk_index).rev() {
            let chunk = self.buf().as_slice()[chunk_index].load(Ordering::Acquire);
            putln!(
                "chunk_index = ",
                chunk_index,
                ", chunk = ",
                Bits::<64>::new(chunk)
            );
            if chunk == 0 {
                steps += BitChunk::BITS as usize;
            } else {
                putln!("trailing_zeros() = ", chunk.trailing_zeros() as usize);
                return Some(steps + chunk.trailing_zeros() as usize);
            }
        }

        None
    }
}

pub struct BitMap<A>
where
    A: Allocator,
{
    buf: UnsafeCell<Unique<[AtomicBitChunk], A>>,
    /// The length itself is obviously stored twice -- but this is actually
    /// quite convenient and not an issue, since we only have one `BitMap` for
    /// the whole program!
    buf_len: AtomicUsize,
    writer_lock: Futex<Private>,
    /// The number of handles to the buffer. We acquire the lock, and then we
    /// have to wait for all the handles to get consumed
    nr_readers: Futex<Private>,
}

impl<A> fmt::Debug for BitMap<A>
where
    A: Allocator,
{
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
unsafe impl<A> Sync for BitMap<A> where A: Allocator {}

/// This futex represents a lock and is unlocked
const FUTEX_UNLOCKED: u32 = 0;

/// THis futex represents a lock and is locked
const FUTEX_LOCKED: u32 = 1;

/// A thread-safe, locking-only-when-necessary bitmap
impl<A> BitMap<A>
where
    A: Allocator,
{
    /// The minimum size of the bitvec (in chunks)
    /// TODO: increase to something less tiny
    pub const MIN_CAPACITY: usize = 16;

    /// # Safety
    ///
    /// `DlMalloc` must be registered as the global allocator *or* a `DlMalloc`
    /// derivative (e.g. `DlMallocMeta`)
    pub const unsafe fn new(allocator: A) -> Self {
        Self {
            buf: UnsafeCell::new(Unique::empty_slice(allocator)),
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
    pub fn acquire_read_guard(&self) -> ReadGuard<A> {
        // putln!("acquire_read_guard()");
        ReadGuard::new(self)
    }

    /// Acquire a reader lock that you have to manually release -- use
    /// `acquire_read_guard()` for a `Drop` guard instead.
    fn inc_nr_readers(&self) {
        putln!("inc_nr_readers()");
        loop {
            // Fast path should stay in userspace!
            _ = self.writer_lock.wait(FUTEX_LOCKED);
            putln!("inc_nr_readers(): finished waiting on writer_lock");
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
        putln!("inc_nr_readers(): finished");
    }

    /// `dec_nr_readers()` is used to release a reader lock, and is carried out
    /// as part of the drop method for `ReadGuard`. Conceptually we do the
    /// following atomic steps
    ///
    /// 1. Decrement the number of readers
    /// 2. If there are no more readers, wake the writer who is waiting on
    ///    readers. All other writers should be waiting on the `writer_lock`
    fn dec_nr_readers(&self) {
        putln!("dec_nr_readers()");
        putln!(
            "dec_nr_readers(): start: nr_readers = ",
            self.nr_readers.value.load(Ordering::Acquire) as usize
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
        putln!(
            "dec_nr_readers(): end: nr_readers = ",
            self.nr_readers.value.load(Ordering::Acquire) as usize
        );
    }

    /// Wait for all readers to finish whatever they're doing. You **must** have
    /// the `writer_lock` for this method to make sense.
    fn wait_on_readers(&self) {
        // putln!("wait_on_readers()");
        // We know that `nr_readers` cannot increase again here, since the
        // caller has asserted they have the `writer_lock`, so we don't have to
        // worry about `inc_nr_readers()` inserting an evil instruction here
        loop {
            let nr_readers = self.nr_readers.value.load(Ordering::Acquire);
            putln!("wait_on_readers(): nr_readers = ", nr_readers as usize);
            if nr_readers == 0 {
                // if nr_readers is already 0, we can safely break (it is capped
                // at 0)
                putln!("wait_on_readers(): nr_readers = 0, done waiting!");
                break;
            }
            // Wait until we update nr_readers to something other than the
            // current value -- if the update was so fast that we never had any
            // futex contention, keep busywaiting, otherwise go to sleep
            putln!("wait_on_readers(): nr_readers.wait_for_update()");
            _ = self.nr_readers.wait(nr_readers);
        }
    }

    /// Acquire a [`BitHandle`] to the bit at bit-index `index`. Note that you
    /// **must** drop this as soon as you can to reduce lock contention.
    ///
    /// # Stuck
    ///
    /// - While the [`BitHandle`] exists!
    pub fn get(&self, index: usize) -> BitHandle<A> {
        // thread_println!("get({index})");
        BitHandle::new(self, index)
    }

    /// Set the bit at `index` to `1`, allocate extra space if we can't fit it
    /// in the current buffer, see struct-level documentation for the allocation
    /// strategy
    pub fn set_high(&self, index: usize) {
        // thread_println!("set_high({index})");
        self.get(index).set_high();
    }

    /// Set the bit at `index` to `0`, allocate extra space if we can't fit it
    /// in the current buffer, see struct-level documentation for the allocation
    /// strategy
    pub fn set_low(&self, index: usize) {
        // thread_println!("set_low({index})");
        self.get(index).set_low();
    }

    /// Attempt to lock this `BitMap` for writer access. If another writer holds
    /// the lock already, we wait until they are done with it. It is **not
    /// sound** to assume you have the lock unless `true` is returned. You must
    /// keep retrying until this function returns `true`. Attempting to acquire
    /// the lock after a successful acquisition will result in deadlock.
    fn try_acquire_writer_lock(&self) -> bool {
        // thread_println!("try_acquire_writer_lock()");
        // Try and acquire the lock, if successful, break;
        if let Ok(FUTEX_UNLOCKED) = self.writer_lock.value.compare_exchange(
            FUTEX_UNLOCKED,
            FUTEX_LOCKED,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            fence(Ordering::Acquire);
            // thread_println!("try_acquire_writer_lock(): acquired lock");
            true
        } else {
            // Wait, if the lock is acquired by another thread
            _ = self.writer_lock.wait(FUTEX_LOCKED);
            false
        }
    }

    /// I've finished writing... okay, other writers can write now!
    fn release_writer_lock(&self) {
        fence(Ordering::Release);
        self.writer_lock
            .value
            .store(FUTEX_UNLOCKED, Ordering::Release);
        // Loads of writers are going to be waiting on this -- we wake them all
        // and _hopefully_ they all realise that they don't need to allocate!
        // see Self::grow() for more comments...
        self.writer_lock.wake(i32::MAX);
    }

    /// Check if the chunk_index at the specified index is within bounds
    pub fn chunk_in_bounds(&self, chunk_index: usize) -> bool {
        self.buf_len.load(Ordering::Acquire) > chunk_index
    }

    /// Grow this bit map to fit a chunk at a certain index (this is a chunk!,
    /// not a bit). The bit-map will eagerly grow exponentially...
    ///
    /// TODO: perhaps that growth strategy is not to be desired??
    pub fn grow(&self, to_fit_chunk_at: usize) {
        putln!("grow(", to_fit_chunk_at, ")");
        let chunk_index = to_fit_chunk_at;
        loop {
            // We can only grow, so we know that this is definitely true
            if self.chunk_in_bounds(chunk_index) {
                // putln!("grow(): {chunk_index} already in bounds -- returning!");
                return;
            }
            putln!("grow(): try_acquire_writer_lock()");
            // This is a continuation of the comments in
            // `try_acquire_writer_lock()`
            if self.try_acquire_writer_lock() {
                putln!("grow(): got writer_lock!");
                break;
            }
        }
        putln!("grow(): wait_on_readers()");
        self.wait_on_readers();

        // SAFETY: We have exclusive access *and* no other references can exist
        // since we wait for all readers to finish.
        let buf = unsafe { &mut *self.buf.get() };
        let sz: usize = cmp::max(1 << log2ceil(chunk_index as u64 + 1), Self::MIN_CAPACITY);
        putln!("grow(): buf.grow(", sz, ")");
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

const BUF_SIZE: usize = MAX_NR_CHARS_USIZE * 3
    + "BitMap { ".len()
    + "writer_lock: ,".len()
    + "buf_len: ,".len()
    + "nr_readers: ".len()
    + " }".len()
    + 32; // for good measure #secure

impl<'a, A> AsciiRepr<'a> for BitMap<A>
where
    A: Allocator,
{
    type AsciiIter = ArrayIter<u8, BUF_SIZE>;

    fn chars(&'a self) -> Self::AsciiIter {
        let mut buf = Cursor::new([0; BUF_SIZE]);
        _ = buf.write(b"BitMap { writer_lock: ");
        write_usize(
            &mut buf,
            self.writer_lock.value.load(Ordering::Acquire) as usize,
        );
        _ = buf.write(b", nr_readers: ");
        write_usize(
            &mut buf,
            self.nr_readers.value.load(Ordering::Acquire) as usize,
        );
        _ = buf.write(b", buf_len: ");
        write_usize(&mut buf, self.buf_len.load(Ordering::Acquire));
        _ = buf.write(b" }");
        ArrayIter {
            array: *buf.get_ref(),
            i: 0,
            end: buf.position() as usize + 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::index_offset;

    #[test]
    fn index_offset_works() {
        assert_eq!(index_offset(42), (0, 42));
        assert_eq!(index_offset(120), (1, 56));
    }
}
