use std::ffi::{c_char, CStr};

extern "C" {
    fn puts(s: *const c_char);
}

/// `puts`... but safe (doesn't use the allocator)
pub fn putstr(s: &CStr) {
    unsafe { puts(s.as_ptr()) }
}

/// Basically just `println!()` but chucks the thread name in front of it!
#[macro_export]
macro_rules! thread_println {
    ($($arg:tt)*) => {
        println!(
            "[{}] {}", 
            ::std::thread::current().name().unwrap_or("{unknown}"), 
            format_args!($($arg)*)
        )
    };
}
