use std::{
    alloc::{GlobalAlloc, Layout},
    ffi::{c_char, CStr},
};

use libc::{c_void, size_t};

use crate::{
    alloc::{
        dlmalloc::DlMalloc,
        tracing::{AllocId, FindPointerMethod, TracingAlloc},
    },
    graph::AllocGraph,
};

#[no_mangle]
pub static LHLMALLOC: TracingAlloc<DlMalloc> =
    unsafe { TracingAlloc::new(DlMalloc::new(), DlMalloc::new()) };

#[no_mangle]
pub unsafe extern "C" fn lhlmalloc(size: size_t, align: size_t) -> *mut c_void {
    LHLMALLOC.alloc(Layout::from_size_align_unchecked(size, align)) as _
}

#[no_mangle]
pub unsafe extern "C" fn lhlfree(ptr: *mut c_void) {
    LHLMALLOC.dealloc(ptr as _, Layout::new::<i32>())
}

#[no_mangle]
pub unsafe extern "C" fn lhl_graph_from_root(
    ptr: *const c_void,
    size: size_t,
    path: *const c_char,
) {
    AllocGraph::from_alloc_id(
        &LHLMALLOC,
        &AllocId {
            size,
            ptr: ptr as _,
        },
        FindPointerMethod::AlignedHeapOnly,
    )
    .write_to_file(CStr::from_ptr(path as _).to_str().unwrap())
    .expect("File exists")
}
