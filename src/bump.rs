use std::alloc::{GlobalAlloc, Layout};
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

const ARENA_SIZE: usize = 128 * 1024;

pub struct BumpAlloc {
    arena: UnsafeCell<[u8; ARENA_SIZE]>,
    next: AtomicUsize,
    allocations: AtomicUsize,
}

impl BumpAlloc {
    pub const fn new() -> Self {
        Self {
            arena: UnsafeCell::new([0x00; ARENA_SIZE]),
            next: AtomicUsize::new(ARENA_SIZE),
            allocations: AtomicUsize::new(0),
        }
    }
}

// trust me
unsafe impl Sync for BumpAlloc {}

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.pad_to_align().size();
        let remaining = self.next.load(Ordering::SeqCst);
        if remaining < size {
            return ptr::null_mut();
        }

        // this also serves as the start pointer since we're allocating from the end
        let remaining_after_alloc = remaining - size;
        self.next.store(remaining_after_alloc, Ordering::SeqCst);
        self.allocations.fetch_add(1, Ordering::SeqCst);

        (self.arena.get() as *mut u8).add(remaining_after_alloc)
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        self.allocations.fetch_sub(1, Ordering::SeqCst);
        if self.allocations.load(Ordering::SeqCst) == 0 {
            self.next.store(ARENA_SIZE, Ordering::SeqCst);
        }
    }
}
