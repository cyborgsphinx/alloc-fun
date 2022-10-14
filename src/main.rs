use alloc_fun::null::NullAlloc;

#[global_allocator]
static ALLOC: NullAlloc = NullAlloc::new();

fn main() {
    println!("Hello, world!");
}
