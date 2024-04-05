use std::{
    alloc::{AllocError, Allocator},
    cell::Cell,
    ptr::{self, NonNull},
    sync::LazyLock,
};

use libc::{
    __errno_location, mmap, munmap, EACCES, EAGAIN, EBADF, EINVAL, ENFILE, ENOMEM, MAP_ANONYMOUS,
    MAP_NORESERVE, MAP_PRIVATE, PROT_READ, PROT_WRITE,
};

use crate::{putln, util::{hint::cold, num::round_up}};

static PAGE_SIZE: LazyLock<usize> = LazyLock::new(page_size::get);

pub struct MmapRegion {
    ptr: *mut u8,
    size: usize,
}

impl MmapRegion {
    /// `mmap()` a read/write, page-aligned arena of memory of `size` bytes (or
    /// more -- the resulting arena is guaranteed to allocated pages). This is
    /// guaranteed to allocate at least 1 page.
    ///
    /// # Panics
    /// - If we can't do the mapping for whatever reason
    pub fn map_noreserve(size: usize) -> MmapRegion {
        if size == 0 {
            panic!("Attmempted to mmap() 0 bytes")
        }
        let size = round_up(size, *PAGE_SIZE);
        // SAFETY: TODO
        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_NORESERVE | MAP_ANONYMOUS | MAP_PRIVATE,
                -1,
                0,
            ) as *mut u8
        };
        if ptr as isize == -1 {
            // super duper unlikely
            cold(|| {
                panic!(
                    "Could not mmap(), errno() = {}",
                    match unsafe { *__errno_location() } {
                        ENOMEM => "ENOMEM", // It's basically just this one
                        EACCES => "EACCESS",
                        EAGAIN => "EAGAIN",
                        EBADF => "EBADF",
                        EINVAL => "EINVAL",
                        ENFILE => "ENFILE",
                        _ => "unknown",
                    }
                );
            });
        }
        MmapRegion { ptr, size }
    }
}

impl Drop for MmapRegion {
    fn drop(&mut self) {
        // SAFETY:
        // - The address addr must be a multiple of the page size (but length
        //   need not be). This is guaranteed true because we can only construct
        //   this region as page-aligned
        // - All pages containing a part of the indicated range are unmapped.
        //   This is fine, because we requested a region of at least `self.size`
        //   bytes and never mutated it.
        unsafe {
            munmap(self.ptr as _, self.size);
        }
    }
}

/// A simple stack allocator, that caps out at the top of a `MmapRegion`,
/// panicking if there is ever not enough space remaining. The benefits of using
/// this are that you can use it even when the GHL is acquired.
///
/// `deallocate()` is a nop and `allocate()` is just a sp bump.
pub struct StackAlloc {
    arena: MmapRegion,
    sp: Cell<usize>,
}

impl StackAlloc {
    /// Construct a new [`StackAlloc`], that can hold up to `capacity` bytes.
    pub fn new(capacity: usize) -> Self {
        putln!("StackAlloc::new(", capacity, ")");
        Self {
            arena: MmapRegion::map_noreserve(capacity),
            sp: Cell::new(0),
        }
    }
}

unsafe impl Allocator for StackAlloc {
    fn allocate(
        &self,
        layout: std::alloc::Layout,
    ) -> Result<ptr::NonNull<[u8]>, std::alloc::AllocError> {
        self.sp.set(round_up(self.sp.get(), layout.align()));
        if self.arena.size - self.sp.get() >= layout.size() {
            // SAFETY:
            // - `layout.size()` > 1
            // - self.arena.size - self.sp.get() >= `layout.size()` >= 1
            // - therefore `self.sp.get() < self.arena.size` (we are in bounds)
            let data = unsafe { self.arena.ptr.add(self.sp.get()) };
            let data = ptr::slice_from_raw_parts_mut(data, layout.size());
            self.sp.set(self.sp.get() + layout.size());
            Ok(unsafe { NonNull::new_unchecked(data) })
        } else {
            Err(AllocError)
        }
    }

    unsafe fn deallocate(&self, _: ptr::NonNull<u8>, _: std::alloc::Layout) {}
}

#[cfg(test)]
mod tests {
    use super::StackAlloc;

    #[test]
    fn stack_allocator_can_be_used_on_box() {
        let alloc = StackAlloc::new(100);
        let alloc = &alloc;
        let n = Box::new_in(42, alloc);
        assert_eq!(*n, 42);
    }
}
