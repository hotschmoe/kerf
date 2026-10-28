//! Size-class allocator over `memory.grow` for the freestanding module (one thread, no imports).
//! Blocks are powers of two from 16 bytes; freed blocks go on a per-class list and are reused.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

const MIN_SHIFT: usize = 4;
const CLASSES: usize = 28;
const PAGE: usize = 65536;

unsafe extern "C" {
    static __heap_base: u8;
}

struct State {
    free: [usize; CLASSES],
    top: usize,
    end: usize,
}

pub struct Heap(UnsafeCell<State>);

// The module is single-threaded.
unsafe impl Sync for Heap {}

#[global_allocator]
static HEAP: Heap = Heap(UnsafeCell::new(State { free: [0; CLASSES], top: 0, end: 0 }));

fn class_of(layout: Layout) -> Option<usize> {
    let size = layout.size().max(layout.align()).max(1 << MIN_SHIFT);
    let shift = size.checked_next_power_of_two()?.trailing_zeros() as usize;
    (shift - MIN_SHIFT < CLASSES).then_some(shift - MIN_SHIFT)
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let Some(c) = class_of(layout) else { return core::ptr::null_mut() };
        // SAFETY: single-threaded access.
        let st = unsafe { &mut *self.0.get() };
        let head = st.free[c];
        if head != 0 {
            // SAFETY: a free block stores the next free block's address in its first word.
            st.free[c] = unsafe { *(head as *const usize) };
            return head as *mut u8;
        }
        let size = 1usize << (c + MIN_SHIFT);
        if st.top == 0 {
            st.top = (core::ptr::addr_of!(__heap_base) as usize + 15) & !15;
            st.end = core::arch::wasm32::memory_size(0) * PAGE;
        }
        let start = (st.top + 15) & !15;
        let Some(stop) = start.checked_add(size) else { return core::ptr::null_mut() };
        if stop > st.end {
            let pages = (stop - st.end).div_ceil(PAGE);
            if core::arch::wasm32::memory_grow(0, pages) == usize::MAX {
                return core::ptr::null_mut();
            }
            st.end += pages * PAGE;
        }
        st.top = stop;
        start as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let Some(c) = class_of(layout) else { return };
        // SAFETY: single-threaded access; the block is at least one word.
        unsafe {
            let st = &mut *self.0.get();
            *(ptr as *mut usize) = st.free[c];
            st.free[c] = ptr as usize;
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller guarantees a valid layout and non-zero new size.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        if class_of(layout) == class_of(new_layout) {
            return ptr;
        }
        // SAFETY: standard grow-copy-free.
        unsafe {
            let new = self.alloc(new_layout);
            if !new.is_null() {
                core::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
            new
        }
    }
}
