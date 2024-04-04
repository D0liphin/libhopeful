/// Read a single `usize` from `src`. `src` can have **no provenance**
///
/// # Safety
/// - This functions is always safe, because the SIGSEGV you might get is up to
///   the arch ;)
#[inline(always)]
pub unsafe fn u8_raw_load_acquire(dst: *mut u8, src: *const u8) {
    use std::arch::asm;

    debug_assert!(src.is_aligned());
    debug_assert!(!src.is_null());

    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm! {
            "mov al, [{src}]",
            "mov [{dst}], al",
            src = in(reg) src,
            dst = in(reg) dst,
            out("al") _,
            options(nostack, preserves_flags),
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    compile_error!("unsupported arch");
}

#[inline(always)]
pub unsafe fn usize_raw_load_acquire(dst: &mut usize, src: *const usize) {
    use std::arch::asm;

    debug_assert!(src.is_aligned());
    debug_assert!(!src.is_null());

    #[cfg(target_arch = "x86_64")]
    unsafe {
        // In x86, things are properly ordered by default, and these operations
        // are atomic!
        asm! {
            "mov rax, [{src}]",
            "mov [{dst}], rax",
            src = in(reg) src,
            dst = in(reg) dst,
            out("rax") _,
            options(nostack, preserves_flags),
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    compile_error!("unsupported arch");
}

/// Read a single `usize` from `src`. `src` can have **no provenance**
///
/// # Safety
/// - This functions is always safe, because the SIGSEGV you might get is up to
///   the arch ;)
/// - Okay, I'm kidding, you can only do this if you are not accessing an object
///   that has a `noalias` marker on it... 
/// - oh wait -- you totally can... nobody cares. 
///
/// # Panics
/// - In debug mode if `src` is `NULL`
/// - In debug mode if `src` is misaligned
#[inline(always)]
pub unsafe fn usize_load_acq(src: *const usize) -> usize {
    let mut dst = 0;
    usize_raw_load_acquire(&mut dst, src);
    dst
}

/// Have you ever wanted to `memcpy()` absolute *garbage*?
/// Are you tired of always having to worry about "provenance" and "the abstract
/// machine"?
/// Try `memcpy_maybe_garbage()` today! 
/// 
/// # Safety
/// 
/// ## Things that `memcpy_maybe_garbage()` can definitely do that `memcpy()` can't
/// 
/// - 
/// 
/// Ok that's enough for that list, moving onto the rest of the stuff! 
/// 
/// ## Things that `memcpy_maybe_garbage()` can maybe do that `memcpy()` maybe can't
/// 
/// - Pass off uninit memory as init
/// - Read from objects that have `noalias` pointers attached to them
/// 
/// That being said, I wouldn't just ignore all the things we know about Rust's
/// UB. Only do these things if you really need to!
#[inline(always)]
pub unsafe fn memcpy_maybe_garbage(dst: *mut u8, src: *const u8, count: usize) {
    for i in 0..count {
        u8_raw_load_acquire(dst.add(i), src.add(i));
    }
}