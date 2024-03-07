use std::ffi::{c_char, CStr};

extern "C" {
    fn puts(s: *const c_char);
}

/// `puts`... but safe (doesn't use the allocator)
pub fn putstr(s: &CStr) {
    unsafe { puts(s.as_ptr()) }
}
