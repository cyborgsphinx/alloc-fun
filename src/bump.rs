use std::alloc::{GlobalAlloc, Layout};
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const DEFAULT_SIZE: usize = 128 * 1024;

pub struct BumpAlloc<const SIZE: usize> {
    arena: UnsafeCell<[u8; SIZE]>,
    next: AtomicUsize,
    allocations: AtomicUsize,
}

impl<const SIZE: usize> BumpAlloc<SIZE> {
    pub const fn new() -> Self {
        Self {
            arena: UnsafeCell::new([0x00; SIZE]),
            next: AtomicUsize::new(SIZE),
            allocations: AtomicUsize::new(0),
        }
    }

    #[cfg(test)]
    fn num_allocated(&self) -> usize {
        self.allocations.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    fn is_clear(&self) -> bool {
        self.next.load(Ordering::SeqCst) == SIZE
    }
}

// trust me
unsafe impl<const SIZE: usize> Sync for BumpAlloc<SIZE> {}

unsafe impl<const SIZE: usize> GlobalAlloc for BumpAlloc<SIZE> {
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

    // concern: we can enter a state where space is allocated and then the next pointer is reset.
    // this would allow us to hand out the same memory twice. which is bad.
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        self.allocations.fetch_sub(1, Ordering::SeqCst);
        if self.allocations.load(Ordering::SeqCst) == 0 {
            self.next.store(SIZE, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_allocates() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let bytes = unsafe { bump.alloc(layout) };
        assert!(!bytes.is_null());
    }

    #[test]
    fn bump_provides_distinct_allocations() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let bytes_1 = unsafe { bump.alloc(layout) };
        let bytes_2 = unsafe { bump.alloc(layout) };
        assert!(!ptr::eq(bytes_1, bytes_2));
    }

    #[test]
    fn bump_holds_allocations() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        // used to ensure the allocator doesn't clear allocated memory
        let _bytes_0 = unsafe { bump.alloc(layout) };
        let bytes_1 = unsafe { bump.alloc(layout) };
        unsafe { bump.dealloc(bytes_1, layout) };
        let bytes_2 = unsafe { bump.alloc(layout) };
        assert!(!ptr::eq(bytes_1, bytes_2));
    }

    #[test]
    fn bump_frees_allocations() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let bytes_1 = unsafe { bump.alloc(layout) };
        unsafe { bump.dealloc(bytes_1, layout) };
        let bytes_2 = unsafe { bump.alloc(layout) };
        assert!(ptr::eq(bytes_1, bytes_2));
    }

    #[test]
    fn bump_may_be_thread_safe() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let values = std::thread::scope(|scope| {
            let mut handles = vec![];
            for _ in 0..100 {
                let handle = scope.spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    let bytes = unsafe { bump.alloc(layout) };
                    bytes as usize
                });
                handles.push(handle);
            }

            let mut values = vec![];
            for handle in handles {
                values.push(handle.join().expect("A thread panicked"));
            }
            values
        });

        for (i, value_i) in values.iter().enumerate() {
            for (j, value_j) in values.iter().enumerate() {
                if i != j {
                    assert_ne!(value_i, value_j);
                }
            }
        }
    }

    #[test]
    fn bump_may_maintain_allocations() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let mut bytes = unsafe { bump.alloc(layout) } as usize;
        for _ in 0..1000 {
            bytes = std::thread::scope(|scope| {
                let dealloc = scope.spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    unsafe { bump.dealloc(bytes as *mut u8, layout) };
                });
                let alloc = scope.spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    let bytes = unsafe { bump.alloc(layout) };
                    bytes as usize
                });
                let bytes = alloc.join().expect("Allocation failed");
                let _ = dealloc.join().expect("Deallocation failed");
                bytes
            });
            assert_eq!(bump.num_allocated(), 1);
            assert!(!bump.is_clear());
        }
    }
}
