use std::alloc::{GlobalAlloc, Layout};
use std::ptr;

pub struct NullAlloc {
}

impl NullAlloc {
    pub const fn new() -> Self {
        Self {
        }
    }
}

unsafe impl GlobalAlloc for NullAlloc {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
    }
}
