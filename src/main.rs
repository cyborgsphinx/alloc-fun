#![cfg(not(test))]

use alloc_fun::bump::{BumpAlloc, DEFAULT_SIZE};

#[global_allocator]
static ALLOC: BumpAlloc::<DEFAULT_SIZE> = BumpAlloc::new();

fn main() {
    println!("Hello, world!");
}
