use alloc_fun::bump::BumpAlloc;

#[global_allocator]
static ALLOC: BumpAlloc = BumpAlloc::new();

fn main() {
    println!("Hello, world!");
}
