//! wasm32 entry points: the raw ABI of SPEC 13.1 (no imports, no wasm-bindgen).
//!
//!   memory
//!   kerf_alloc(len) -> ptr        kerf_free(ptr, len)
//!   kerf_call(fn_ptr, fn_len, in_ptr, in_len) -> 0 ok | 1 error (output is error JSON)
//!   kerf_out_ptr() / kerf_out_len()   (valid until the next call)

#![no_std]

extern crate alloc;

mod heap;

use alloc::alloc::{Layout, alloc, dealloc};
use alloc::vec::Vec;
use core::cell::UnsafeCell;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

struct Output(UnsafeCell<Vec<u8>>);

// The module is single-threaded; the host calls one export at a time.
unsafe impl Sync for Output {}

static OUT: Output = Output(UnsafeCell::new(Vec::new()));

fn layout(len: u32) -> Layout {
    let size = (len as usize).max(1);
    // Align 1 is always valid for a non-zero size below isize::MAX.
    Layout::from_size_align(size, 1).unwrap_or(Layout::new::<u8>())
}

#[unsafe(no_mangle)]
pub extern "C" fn kerf_alloc(len: u32) -> u32 {
    // SAFETY: the layout is non-zero sized.
    unsafe { alloc(layout(len)) as usize as u32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn kerf_free(ptr: u32, len: u32) {
    if ptr == 0 {
        return;
    }
    // SAFETY: `ptr` came from `kerf_alloc(len)` with the same layout.
    unsafe { dealloc(ptr as usize as *mut u8, layout(len)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn kerf_call(fn_ptr: u32, fn_len: u32, in_ptr: u32, in_len: u32) -> i32 {
    // SAFETY: the host passes ranges it wrote into buffers from `kerf_alloc`.
    let (name, input) = unsafe {
        (
            core::slice::from_raw_parts(fn_ptr as usize as *const u8, fn_len as usize),
            core::slice::from_raw_parts(in_ptr as usize as *const u8, in_len as usize),
        )
    };
    let result = kerf_core::api::call(name, input);
    // SAFETY: single-threaded, no outstanding borrow of OUT.
    let out = unsafe { &mut *OUT.0.get() };
    *out = result.bytes;
    if result.ok { 0 } else { 1 }
}

#[unsafe(no_mangle)]
pub extern "C" fn kerf_out_ptr() -> u32 {
    // SAFETY: single-threaded read.
    unsafe { (*OUT.0.get()).as_ptr() as usize as u32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn kerf_out_len() -> u32 {
    // SAFETY: single-threaded read.
    unsafe { (*OUT.0.get()).len() as u32 }
}
