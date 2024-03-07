// use core::ffi::c_size_t;
// use std::{
//     alloc::{AllocError, Allocator, GlobalAlloc, Layout},
//     ffi::c_void,
//     io::{stdout, Write},
//     ptr::{self, NonNull},
// };

// use crate::{
//     alloc::meta::{AllocMeta, AllocMetaIndex},
//     lazy_lock::{LazyLock, LazyLockState},
//     util::{hint::cold, print::putstr},
// };

// use super::manual::{free, malloc, xmalloc};

// /// Offsets into `ALLOC_META_INDEX` for a given allocation
// pub(crate) static ALLOC_META_INDEX: LazyLock<AllocMetaIndex, fn() -> AllocMetaIndex> =
//     LazyLock::new(AllocMetaIndex::new);

// pub struct TracingAlloc;

// pub fn ascribe_meta(data: *mut [u8], layout: Layout) {
//     let meta = AllocMeta::from_layout_without_annotations(layout);
//     let meta: &'static AllocMeta = if let Some(meta) = ALLOC_META_INDEX.is_interned(&meta) {
//         meta
//     } else {
//         xmalloc(meta).leak()
//     };
//     putstr(c"  acquired &'static meta object");
//     ALLOC_META_INDEX.insert(data as *const u8 as usize, meta);
// }

// pub fn remove_meta(data: *mut u8) {
//     ALLOC_META_INDEX.remove(data as _);
// }

// unsafe impl Allocator for TracingAlloc {
//     fn allocate(
//         &self,
//         layout: std::alloc::Layout,
//     ) -> Result<std::ptr::NonNull<[u8]>, std::alloc::AllocError> {
//         self.allocate_zeroed(layout)
//     }

//     fn allocate_zeroed(
//         &self,
//         layout: std::alloc::Layout,
//     ) -> Result<std::ptr::NonNull<[u8]>, std::alloc::AllocError> {
//         let data = malloc(layout)?;
//         if LazyLock::state(&ALLOC_META_INDEX) == LazyLockState::Init {
//             putstr(c"ALLOC_META_INDEX init, so ascribing meta...");
//             ascribe_meta(data.as_ptr(), layout);
//         } else if LazyLock::state(&ALLOC_META_INDEX) == LazyLockState::Uninit {
//             putstr(c"ALLOC_META_INDEX is uninit, initializing...");
//             cold(|| {
//                 let ll = LazyLock::initialize(&ALLOC_META_INDEX);
//                 putstr(c"  initialized zero-sized ALLOC_META_INDEX");
//                 ll
//             });
//         }
//         Ok(data)
//     }

//     unsafe fn deallocate(&self, ptr: std::ptr::NonNull<u8>, _: std::alloc::Layout) {
//         free(ptr.as_ptr());
//         remove_meta(ptr.as_ptr())
//     }
// }

// unsafe impl GlobalAlloc for TracingAlloc {
//     unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
//         self.alloc_zeroed(layout)
//     }

//     unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
//         match self.allocate(layout) {
//             Ok(data) => data.as_ptr() as _,
//             Err(e) => {
//                 panic!("Allocation error: {:?}", e);
//             }
//         }
//     }

//     unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
//         // SAFETY: Caller is required to pass a pointer allocated by this
//         //         allocator. Since we do not accept the null pointer as a valid
//         //         allocation, we can assert this never happens.
//         let ptr = NonNull::new_unchecked(ptr);
//         // SAFETY: Identical safety contract as caller.
//         self.deallocate(ptr, layout);
//     }
// }
