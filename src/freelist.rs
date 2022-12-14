use std::alloc::{GlobalAlloc, Layout};
use std::mem;
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

    #[cfg(test)]
    fn free_list_length(&self) -> usize {
        self.details.lock().unwrap().free_list_length()
    }

    #[cfg(test)]
    fn free_space(&self) -> usize {
        self.details.lock().unwrap().free_space()
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
    fn new(size: usize) -> Self {
        Self { size, next: None }
    }

    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    fn suitable_for(&self, size: usize) -> bool {
        // check that the size at this node is enough for the allocation request
        // also check free memory after allocation location for capability to fit a new ListNode
        // either there is no free space left, in which case we don't need to add a new node,
        // or we must fit a new node into the remaining space so that we don't lose track of it
        size <= self.size
            && self
                .size
                .checked_sub(size)
                .map(|excess| excess == 0 || excess >= mem::size_of::<ListNode>())
                .unwrap_or(false)
    }
}

struct FreeListImpl<const SIZE: usize> {
    arena: [u8; SIZE],
    // using option to indicate when initialization has happened
    // not using an actual initialization function because I'm not sure where to call it yet
    // this also seems to get us off nightly rust
    head: Option<ListNode>,
}

impl<const SIZE: usize> FreeListImpl<SIZE> {
    const fn new() -> Self {
        Self {
            arena: [0x00; SIZE],
            head: None,
        }
    }

    #[cfg(test)]
    fn free_list_length(&self) -> usize {
        let mut length = 0;
        if let Some(ref head) = self.head {
            let mut current = head;
            while let Some(ref node) = current.next {
                length += 1;
                current = node;
            }
        }
        length
    }

    #[cfg(test)]
    fn free_space(&self) -> usize {
        let mut space = 0;
        if let Some(ref head) = self.head {
            let mut current = head;
            while let Some(ref node) = current.next {
                space += node.size;
                current = node;
            }
        }
        space
    }

    fn find_region(&mut self, size: usize) -> Option<&'static mut ListNode> {
        let mut prev = self
            .head
            .as_mut()
            .expect("Free list head must be Some() in find_region");

        while let Some(ref mut region) = prev.next {
            if region.suitable_for(size) {
                let next = region.next.take();
                let ret = prev.next.take();
                prev.next = next;
                return ret;
            } else {
                prev = prev.next.as_mut().unwrap();
            }
        }
        None
    }

    unsafe fn add_free_region(&mut self, addr: *mut u8, size: usize) {
        // ensure that size and alignment of freed space can fit a ListNode object
        assert!(!addr.is_null(), "Cannot free null pointer");
        assert_eq!(
            addr as usize % mem::align_of::<ListNode>(),
            0,
            "Alignment must work for ListNode"
        );
        assert!(
            size >= mem::size_of::<ListNode>(),
            "Freed area must fit a ListNode"
        );

        let mut head = self
            .head
            .as_mut() // don't consume the optional, just modify the value
            .expect("Free list head must be Some() in add_free_region");
        let mut node = ListNode::new(size);
        node.next = head.next.take();
        let node_ptr = addr as *mut ListNode;
        node_ptr.write(node);
        head.next = Some(&mut *node_ptr);
    }

    fn adjust_layout(layout: Layout) -> Layout {
        let size = layout.size().max(mem::size_of::<ListNode>());
        let align = layout.align().max(mem::align_of::<ListNode>());
        Layout::from_size_align(size, align).expect("Could not construct new alignment")
    }

    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        // head is only None before the first alloc call
        if self.head.is_none() {
            self.head = Some(ListNode::new(0));
            let start = self.arena.as_mut_ptr();
            self.add_free_region(start, SIZE);
        }
        let size = Self::adjust_layout(layout).pad_to_align().size();
        if let Some(node) = self.find_region(size) {
            let alloc_start = node as *mut ListNode;
            assert_eq!(alloc_start as usize, node.start_addr());
            assert!(node.size >= size);
            let alloc_end = alloc_start.add(size);
            //if alloc_end as usize > node.end_addr() as usize {
            //    return ptr::null_mut();
            //}
            let excess = node.size - size;
            if excess > 0 {
                self.add_free_region(alloc_end as *mut u8, excess);
            }
            alloc_start as *mut u8
        } else {
            ptr::null_mut()
        }
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        let size = Self::adjust_layout(layout).pad_to_align().size();
        self.add_free_region(ptr, size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begins_with_no_head() {
        let alloc = FreeListImpl::<DEFAULT_SIZE>::new();
        assert_eq!(alloc.free_list_length(), 0);
        assert!(alloc.head.is_none());
    }

    #[test]
    fn can_add_free_region() {
        let mut alloc = FreeListImpl::<DEFAULT_SIZE>::new();
        unsafe {
            alloc.head = Some(ListNode::new(0));
            let start = alloc.arena.as_mut_ptr();
            alloc.add_free_region(start, DEFAULT_SIZE);
        }
        assert_eq!(alloc.free_list_length(), 1);
        assert_eq!(alloc.free_space(), DEFAULT_SIZE);
        assert!(alloc.head.unwrap().next.is_some());
    }

    #[test]
    fn allocates_correct_size() {
        let alloc = FreeListAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let true_size = FreeListImpl::<DEFAULT_SIZE>::adjust_layout(layout)
            .pad_to_align()
            .size();
        let _bytes = unsafe { alloc.alloc(layout) };
        assert_eq!(alloc.free_space(), DEFAULT_SIZE - true_size);
    }

    #[test]
    fn can_allocate_more_than_list_node_size() {
        let alloc = FreeListAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(256, 4).unwrap();
        let true_size = FreeListImpl::<DEFAULT_SIZE>::adjust_layout(layout)
            .pad_to_align()
            .size();
        let _bytes = unsafe { alloc.alloc(layout) };
        assert_eq!(alloc.free_space(), DEFAULT_SIZE - true_size);
    }

    #[test]
    fn can_allocate() {
        let alloc = FreeListAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let bytes = unsafe { alloc.alloc(layout) };
        assert!(!bytes.is_null());
        let more_bytes = unsafe { alloc.alloc(layout) };
        assert!(!more_bytes.is_null());
        assert!(!ptr::eq(bytes, more_bytes));
    }

    #[test]
    fn can_deallocate() {
        let alloc = FreeListAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        let bytes = unsafe { alloc.alloc(layout) };
        unsafe { alloc.dealloc(bytes, layout) };
        assert_eq!(alloc.free_space(), DEFAULT_SIZE);
    }

    #[test]
    fn repeated_alloc_and_dealloc_keeps_free_list_stable() {
        let alloc = FreeListAlloc::<DEFAULT_SIZE>::new();
        let layout = Layout::from_size_align(10, 4).unwrap();
        for _ in 0..100 {
            unsafe {
                let bytes = alloc.alloc(layout);
                alloc.dealloc(bytes, layout);
            }
            assert_eq!(alloc.free_list_length(), 2);
        }
    }

    #[test]
    fn can_skip_values_in_free_list() {
        let alloc = FreeListAlloc::<DEFAULT_SIZE>::new();
        let layout1 = Layout::from_size_align(10, 4).unwrap();
        let layout2 = Layout::from_size_align(256, 4).unwrap();
        unsafe {
            let bytes = alloc.alloc(layout1);
            alloc.dealloc(bytes, layout1);
            let _bytes = alloc.alloc(layout2);
        }
        assert_eq!(
            alloc.free_space(),
            DEFAULT_SIZE - layout2.pad_to_align().size()
        );
    }
}
