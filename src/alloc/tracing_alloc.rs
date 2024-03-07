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

use core::ffi::c_size_t;
use std::{
    alloc::{AllocError, Allocator, GlobalAlloc, Layout},
    ffi::c_void,
    io::{stdout, Write},
    ptr::{self, NonNull},
};

use crate::{
    alloc_meta::{AllocMeta, AllocMetaIndex},
    lazy_lock::{LazyLock, LazyLockState},
    util::{hint::cold, print::putstr},
};