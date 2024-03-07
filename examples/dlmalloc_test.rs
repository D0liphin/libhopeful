//! We can't test dlmalloc() in a normal test, because we need to specify the
//! global allocator for the entire lib? I think. Not sure how that works 
//! actually. 
//! 
//! TODO: figure that out

#![feature(allocator_api)]

use lhl::alloc::dlmalloc::DlMalloc;

#[global_allocator]
static DLMALLOC: DlMalloc = unsafe {DlMalloc::new()};

fn main() {
    let n = Box::new(5);
    println!("{n}");
}