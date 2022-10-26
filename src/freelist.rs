use std::alloc::{GlobalAlloc, Layout};
use std::ptr;
use std::sync::Mutex;

pub const DEFAULT_SIZE: usize = 128 * 1024;

pub struct FreeListAlloc<const SIZE: usize> {
    details: Mutex<FreeListImpl<SIZE>>,
}

impl<const SIZE: usize> FreeListAlloc<SIZE> {
    pub const fn new() -> Self {
        Self {
            details: Mutex::new(FreeListImpl::<SIZE>::new()),
        }
    }
}

unsafe impl<const SIZE: usize> GlobalAlloc for FreeListAlloc<SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Ok(mut details) = self.details.lock() {
            details.alloc(layout)
        } else {
            ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Ok(mut details) = self.details.lock() {
            details.dealloc(ptr, layout);
        }
    }
}

struct ListNode {
    size: usize,
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    const fn new(size: usize) -> Self {
        Self { size, next: None }
    }

    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

struct FreeListImpl<const SIZE: usize> {
    arena: [u8; SIZE],
    head: ListNode,
}

impl<const SIZE: usize> FreeListImpl<SIZE> {
    const fn new() -> Self {
        Self {
            arena: [0x00; SIZE],
            head: ListNode::new(0),
        }
    }

    unsafe fn alloc(&mut self, _layout: Layout) -> *mut u8 {
        todo!()
    }

    unsafe fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout) {
        todo!()
    }
}
