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

        let mut start = 0;
        if self.next.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |mut remaining| {
            if remaining < size {
                return None;
            }
            remaining -= size;
            start = remaining;
            Some(remaining)
        }).is_err() {
            return ptr::null_mut();
        }

        self.allocations.fetch_add(1, Ordering::SeqCst);
        (self.arena.get() as *mut u8).add(start)
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

    // this must be a static to be shared across threads
    static BUMP: BumpAlloc = BumpAlloc::new();

    #[test]
    fn bump_may_be_thread_safe() {
        let layout = Layout::from_size_align(10, 4).unwrap();
        let mut handles = vec![];
        for _ in 0..100 {
            let layout = layout.clone();
            let handle = std::thread::spawn(move || {
                unsafe {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    let bytes = BUMP.alloc(layout);
                    bytes as usize
                }
            });
            handles.push(handle);
        }

        let mut values = vec![];
        for handle in handles {
            values.push(handle.join().expect("A thread panicked"));
        }

        for i in 0..100 {
            for j in (i+1)..100 {
                assert_ne!(values[i], values[j]);
            }
        }
    }
}
