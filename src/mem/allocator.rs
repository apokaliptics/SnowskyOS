// ═══════════════════════════════════════════════════════════════════════════════
// mem/allocator.rs — Thread-safe Buddy Allocator for `alloc` support
// ═══════════════════════════════════════════════════════════════════════════════
//! A simple buddy-system allocator providing a `#[global_allocator]` suitable
//! for bare-metal Rust with `alloc`.  Thread-safety is achieved via a spinlock
//! so that the audio ISR and the UI main-loop can both allocate safely.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use spin::Mutex;

/// Minimum block size = 32 bytes (covers most small allocs; reduces fragmentation overhead).
const MIN_ORDER: usize = 5; // 2^5 = 32
/// Maximum order — supports up to 2^25 = 32 MB contiguous.
const MAX_ORDER: usize = 25;
/// Number of distinct orders.
const ORDER_COUNT: usize = MAX_ORDER - MIN_ORDER + 1;

// ═════════════════════════════════════════════════════════════════════════════
// Free-list node (intrusive linked-list stored *inside* free blocks)
// ═════════════════════════════════════════════════════════════════════════════
#[repr(C)]
struct FreeNode {
    next: *mut FreeNode,
}

// ═════════════════════════════════════════════════════════════════════════════
// BuddyAllocator inner state
// ═════════════════════════════════════════════════════════════════════════════
struct BuddyInner {
    free_lists: [*mut FreeNode; ORDER_COUNT],
    heap_start: usize,
    heap_size: usize,
    initialised: bool,
}

unsafe impl Send for BuddyInner {}

impl BuddyInner {
    const fn new() -> Self {
        Self {
            free_lists: [ptr::null_mut(); ORDER_COUNT],
            heap_start: 0,
            heap_size: 0,
            initialised: false,
        }
    }

    /// Initialise with the heap region supplied by the linker script.
    fn init(&mut self, start: usize, size: usize) {
        self.heap_start = start;
        self.heap_size = size;
        self.initialised = true;

        // Populate free lists by splitting the whole heap into the largest
        // power-of-two blocks that fit.
        let mut addr = start;
        let end = start + size;

        while addr < end {
            let remaining = end - addr;
            // Find the largest order that fits at this alignment
            let order = self.largest_order(addr, remaining);
            self.push_free(order, addr);
            addr += 1 << (order + MIN_ORDER);
        }
    }

    /// Find largest order block that: fits in `remaining` AND is aligned.
    fn largest_order(&self, addr: usize, remaining: usize) -> usize {
        let mut order = ORDER_COUNT - 1;
        loop {
            let size = 1 << (order + MIN_ORDER);
            if size <= remaining && (addr & (size - 1)) == 0 {
                return order;
            }
            if order == 0 {
                return 0;
            }
            order -= 1;
        }
    }

    /// Push a block onto the free list for `order`.
    fn push_free(&mut self, order: usize, addr: usize) {
        let node = addr as *mut FreeNode;
        unsafe {
            (*node).next = self.free_lists[order];
        }
        self.free_lists[order] = node;
    }

    /// Pop a block from the free list for `order`. Returns null if empty.
    fn pop_free(&mut self, order: usize) -> *mut u8 {
        let node = self.free_lists[order];
        if node.is_null() {
            return ptr::null_mut();
        }
        unsafe {
            self.free_lists[order] = (*node).next;
        }
        node as *mut u8
    }

    /// Allocate a block of at least `layout.size()` bytes with `layout.align()`.
    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(layout.align()).max(1 << MIN_ORDER);
        let order = size_to_order(size);

        // Walk up to find a free block, splitting if necessary
        for o in order..ORDER_COUNT {
            let blk = self.pop_free(o);
            if !blk.is_null() {
                // Split down to the requested order
                let mut current = o;
                while current > order {
                    current -= 1;
                    let buddy_addr = blk as usize + (1 << (current + MIN_ORDER));
                    self.push_free(current, buddy_addr);
                }
                return blk;
            }
        }

        ptr::null_mut() // OOM
    }

    /// Deallocate a block previously returned by `alloc`.
    fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        let size = layout.size().max(layout.align()).max(1 << MIN_ORDER);
        let order = size_to_order(size);
        let mut addr = ptr as usize;
        let mut current_order = order;

        // Attempt to coalesce with buddy
        while current_order < ORDER_COUNT - 1 {
            let block_size = 1 << (current_order + MIN_ORDER);
            let buddy_addr = addr ^ block_size;

            // Check if buddy is in the free list
            if self.remove_free(current_order, buddy_addr) {
                // Merge
                addr = addr.min(buddy_addr);
                current_order += 1;
            } else {
                break;
            }
        }

        self.push_free(current_order, addr);
    }

    /// Remove a specific address from a free list. Returns true if found.
    fn remove_free(&mut self, order: usize, addr: usize) -> bool {
        let target = addr as *mut FreeNode;
        let mut prev: *mut *mut FreeNode = &mut self.free_lists[order];
        let mut current = self.free_lists[order];

        while !current.is_null() {
            if current == target {
                unsafe {
                    *prev = (*current).next;
                }
                return true;
            }
            unsafe {
                prev = &mut (*current).next;
                current = (*current).next;
            }
        }
        false
    }
}

/// Convert a size to a buddy order index.
fn size_to_order(size: usize) -> usize {
    let mut order = 0;
    let mut s = 1 << MIN_ORDER;
    while s < size {
        s <<= 1;
        order += 1;
    }
    order
}

// ═════════════════════════════════════════════════════════════════════════════
// Global allocator wrapper with spinlock
// ═════════════════════════════════════════════════════════════════════════════

struct LockedBuddyAllocator {
    inner: Mutex<BuddyInner>,
}

unsafe impl Sync for LockedBuddyAllocator {}

#[global_allocator]
static ALLOCATOR: LockedBuddyAllocator = LockedBuddyAllocator {
    inner: Mutex::new(BuddyInner::new()),
};

unsafe impl GlobalAlloc for LockedBuddyAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.inner.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.lock().dealloc(ptr, layout);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Init (called from _entry before anything else touches the heap)
// ═════════════════════════════════════════════════════════════════════════════

/// Initialise the global buddy allocator with the heap region.
///
/// # Safety
/// Must be called exactly once, before any `alloc` calls, with a valid
/// memory region that does not overlap `.bss` / `.data` / stack.
pub fn init(heap_start: usize, heap_size: usize) {
    ALLOCATOR.inner.lock().init(heap_start, heap_size);
}
