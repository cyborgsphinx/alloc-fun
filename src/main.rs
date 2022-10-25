#![cfg(not(test))]

use std::boxed::Box;

use alloc_fun::bump::{BumpAlloc, DEFAULT_SIZE};

#[global_allocator]
static ALLOC: BumpAlloc<DEFAULT_SIZE> = BumpAlloc::new();

fn main() {
    let mut outer = Box::new(0);
    for _ in 0..100 {
        let mut val = Box::new(1);
        *val += 1;
    }
    *outer += 1;
    println!("Hello, world!");
}
