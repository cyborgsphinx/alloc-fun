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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_allocates() {
        let bump = BumpAlloc::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        unsafe {
            let bytes = bump.alloc(layout);
            assert!(!bytes.is_null());
        }
    }

    #[test]
    fn bump_provides_distinct_allocations() {
        let bump = BumpAlloc::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        unsafe {
            let bytes_1 = bump.alloc(layout);
            let bytes_2 = bump.alloc(layout);
            assert!(!ptr::eq(bytes_1, bytes_2));
        }
    }

    #[test]
    fn bump_holds_allocations() {
        let bump = BumpAlloc::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        unsafe {
            // used to ensure the allocator doesn't clear allocated memory
            let _bytes_0 = bump.alloc(layout);
            let bytes_1 = bump.alloc(layout);
            bump.dealloc(bytes_1, layout);
            let bytes_2 = bump.alloc(layout);
            assert!(!ptr::eq(bytes_1, bytes_2));
        }
    }

    #[test]
    fn bump_frees_allocations() {
        let bump = BumpAlloc::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        unsafe {
            let bytes_1 = bump.alloc(layout);
            bump.dealloc(bytes_1, layout);
            let bytes_2 = bump.alloc(layout);
            assert!(ptr::eq(bytes_1, bytes_2));
        }
    }
}
