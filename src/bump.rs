use std::alloc::{GlobalAlloc, Layout};
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::Mutex;

pub const DEFAULT_SIZE: usize = 128 * 1024;

pub struct BumpAlloc<const SIZE: usize> {
    arena: UnsafeCell<[u8; SIZE]>,
    details: Mutex<BumpImpl>, // use of a mutex may not be ideal, but it makes handling changes to two values easier
}

impl<const SIZE: usize> BumpAlloc<SIZE> {
    pub const fn new() -> Self {
        Self {
            arena: UnsafeCell::new([0x00; SIZE]),
            details: Mutex::new(BumpImpl::new()),
        }
    }

    #[cfg(test)]
    fn num_allocated(&self) -> usize {
        self.details.lock().unwrap().allocations
    }

    #[cfg(test)]
    fn is_clear(&self) -> bool {
        self.details.lock().unwrap().next == 0
    }
}

// we're handing out non-overlapping chunks of the arena, and the rest is mutex-guarded
unsafe impl<const SIZE: usize> Sync for BumpAlloc<SIZE> {}

unsafe impl<const SIZE: usize> GlobalAlloc for BumpAlloc<SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Ok(mut details) = self.details.lock() {
            let size = layout.pad_to_align().size();
            if details.next + size > SIZE {
                return ptr::null_mut();
            }
            let start = details.next;
            details.next += size;
            details.allocations += 1;
            (self.arena.get() as *mut u8).add(start)
        } else {
            ptr::null_mut()
        }
    }

    // concern: we can enter a state where space is allocated and then the next pointer is reset.
    // this would allow us to hand out the same memory twice. which is bad.
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        if let Ok(mut details) = self.details.lock() {
            details.allocations -= 1;
            if details.allocations == 0 {
                details.next = 0;
            }
        }
    }
}

struct BumpImpl {
    next: usize,
    allocations: usize,
}

impl BumpImpl {
    const fn new() -> Self {
        Self {
            next: 0,
            allocations: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let bytes = unsafe { bump.alloc(layout) };
        assert!(!bytes.is_null());
    }

    #[test]
    fn provides_distinct_allocations() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let bytes_1 = unsafe { bump.alloc(layout) };
        let bytes_2 = unsafe { bump.alloc(layout) };
        assert!(!ptr::eq(bytes_1, bytes_2));
    }

    #[test]
    fn holds_allocations() {
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
    fn frees_allocations() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let bytes_1 = unsafe { bump.alloc(layout) };
        unsafe { bump.dealloc(bytes_1, layout) };
        let bytes_2 = unsafe { bump.alloc(layout) };
        assert!(ptr::eq(bytes_1, bytes_2));
    }

    #[test]
    fn may_be_thread_safe() {
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

    // ignoring due to how long it takes to run in successful cases
    // run this test to check for alloc/dealloc contention
    #[ignore]
    #[test]
    fn may_maintain_allocations() {
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

    #[test]
    fn may_fail_to_allocate() {
        let bump = BumpAlloc::<0>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let bytes = unsafe { bump.alloc(layout) };
        assert!(bytes.is_null());
    }

    #[test]
    fn begins_cleared() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        assert!(bump.is_clear());
    }

    #[test]
    fn begins_with_no_allocations() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        assert_eq!(bump.num_allocated(), 0);
    }

    #[test]
    fn hands_out_non_overlapping_chunks() {
        let bump = BumpAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(8, 4).unwrap();
        unsafe {
            let bytes = bump.alloc(layout);
            let more_bytes = bump.alloc(layout);
            ptr::write_bytes(more_bytes, 0xff, 8);
            ptr::write_bytes(bytes, 0x00, 8);
            let byte = ptr::read(more_bytes);
            assert!(byte == 0xff);
        }
    }
}
