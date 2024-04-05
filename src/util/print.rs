use std::{
    ffi::{c_char, CStr},
    io::Write,
    ops,
};

use libc::putchar;

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
        // println!(
        //     "[{}] {}",
        //     ::std::thread::current().name().unwrap_or("{unknown}"),
        //     format_args!($($arg)*)
        // )
    };
}

/// Again, just println!() but lets you write `Self::method_name` instead of
/// `FullTypeName<Something, Something>::method_name`
#[macro_export]
macro_rules! fcprintln {
    ($($arg:tt)*) => {
        println!("{}:{}", ::tynm::type_name::<Self>(), format_args!($($arg)*))
    };
}

pub trait AsciiRepr<'a> {
    type AsciiIter: Iterator<Item = u8>;
    fn chars(&'a self) -> Self::AsciiIter;
}

impl<'a> AsciiRepr<'a> for &'a str {
    type AsciiIter = std::iter::Copied<std::slice::Iter<'a, u8>>;

    fn chars(&self) -> Self::AsciiIter {
        self.as_bytes().iter().copied()
    }
}

pub struct ArrayIter<T, const SIZE: usize> {
    pub array: [T; SIZE],
    pub i: usize,
    pub end: usize,
}

impl<T, const SIZE: usize> Iterator for ArrayIter<T, SIZE>
where
    T: Copy,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == self.end {
            return None;
        }
        let el = self.array[self.i];
        self.i += 1;
        Some(el)
    }
}

impl<const SIZE: usize> Bits<SIZE> {
    pub fn new(n: impl Into<u128>) -> Self {
        Self { n: n.into() }
    }
}

pub struct Bits<const SIZE: usize> {
    n: u128,
}

impl<'a, const SIZE: usize> AsciiRepr<'a> for Bits<SIZE> {
    type AsciiIter = ArrayIter<u8, { u128::BITS as usize }>;

    fn chars(&'a self) -> Self::AsciiIter {
        let mut array = [0u8; u128::BITS as usize];
        for i in 0..SIZE {
            array[i] = if (self.n & (1 << i)) != 0 { b'1' } else { b'0' }
        }
        ArrayIter {
            array,
            i: 0,
            end: SIZE,
        }
    }
}

impl<'a> AsciiRepr<'a> for usize {
    type AsciiIter = ArrayIter<u8, { MAX_NR_CHARS_USIZE + 1 }>;

    fn chars(&self) -> Self::AsciiIter {
        let mut n = *self;
        let mut result = [0u8; MAX_NR_CHARS_USIZE + 1];
        let mut i = result.len() - 1;
        if n == 0 {
            result[i] = b'0';
            i -= 1;
        } else {
            while n > 0 {
                result[i] = (n % 10) as u8 + b'0';
                n /= 10;
                i -= 1;
            }
        }
        let end = result.len();
        let i = i + 1;
        ArrayIter {
            array: result,
            i,
            end,
        }
    }
}

/// Unbuffered output stream (we don't call malloc). Obviously not quite like
/// C++ as a result...
pub struct COut(());

pub fn cout() -> COut {
    COut(())
}

pub struct Endl(());

static ENDL: &Endl = &Endl(());

pub fn endl() -> &'static Endl {
    ENDL
}

impl<'a> AsciiRepr<'a> for &'a Endl {
    type AsciiIter = ArrayIter<u8, 1>;
    fn chars(&self) -> Self::AsciiIter {
        ArrayIter {
            array: [b'\n'],
            i: 0,
            end: 1,
        }
    }
}

impl<'t, T> ops::Shl<&'t T> for COut
where
    T: AsciiRepr<'t> + ?Sized,
{
    type Output = COut;
    fn shl(self, rhs: &'t T) -> Self::Output {
        for ch in rhs.chars() {
            unsafe {
                putchar(ch as _);
            }
        }
        self
    }
}

impl<'a, 't, T> ops::Shl<&'t T> for &'a mut COut
where
    T: AsciiRepr<'t> + ?Sized,
{
    type Output = &'a mut COut;
    fn shl(self, rhs: &'t T) -> Self::Output {
        for ch in rhs.chars() {
            unsafe {
                putchar(ch as _);
            }
        }
        self
    }
}

#[macro_export]
macro_rules! put {
    ($($e:expr),*$(,)?) => {{
        // _ = $crate::util::print::cout() $(<< &$e)*;
    }};
}

#[macro_export]
macro_rules! putln {
    ($($e:expr),*$(,)?) => {{
        // use $crate::put;
        // _ = $crate::util::print::cout() $(<< &($e))*;
        // put!($crate::util::print::endl());
    }};
}

pub const MAX_NR_CHARS_USIZE: usize = "18446744073709551615".len();

pub fn write_usize(mut buf: impl Write, mut n: usize) {
    let mut result = [0u8; MAX_NR_CHARS_USIZE + 1];
    let mut i = result.len() - 1;
    if n == 0 {
        result[i] = b'0';
        i -= 1;
    } else {
        while n > 0 {
            result[i] = (n % 10) as u8 + b'0';
            n /= 10;
            i -= 1;
        }
    }
    _ = buf.write_all(&result[i + 1..]);
}

#[cfg(test)]
mod tests {
    use super::write_usize;

    #[test]
    fn writes_usizes() {
        let mut nonstdout = vec![];
        write_usize(&mut nonstdout, 14500);
        assert_eq!(nonstdout, Vec::from(b"14500"));

        let mut nonstdout = vec![];
        write_usize(&mut nonstdout, 1450);
        assert_eq!(nonstdout, Vec::from(b"1450"));

        let mut nonstdout = vec![];
        write_usize(&mut nonstdout, 0);
        assert_eq!(nonstdout, Vec::from(b"0"));
    }
}
