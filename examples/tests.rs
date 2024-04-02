//! For now, tests are going to go here, because apparently I can't set a
//! global allocator in a test... who knew!?

#![feature(allocator_api)]

use std::{thread, time};

use lhl::{
    alloc::{bitvec::BitMap, dlmalloc::DlMalloc},
    thread_println,
};

#[global_allocator]
static DLMALLOC: DlMalloc = unsafe { DlMalloc::new() };

static BIT_MAP: BitMap = unsafe { BitMap::new() };

fn main() {
    thread_println!("Hello, world!");
    let guards = [BIT_MAP.acquire_read_guard(), BIT_MAP.acquire_read_guard()];
    thread::Builder::new()
        .name(String::from("reader-1"))
        .spawn(|| {
            thread::sleep(time::Duration::from_millis(2000));
            thread_println!("thread::spawn()");
            drop(guards);
        })
        .unwrap();
    BIT_MAP.set_high(345);
    BIT_MAP.set_high(344);
    dbg!(&BIT_MAP);
}
