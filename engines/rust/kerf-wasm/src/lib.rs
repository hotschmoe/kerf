//! Raw wasm ABI for the Kerf engine (SPEC section 13.1). No wasm-bindgen, no imports.
//!
//! Exports: `memory`, `kerf_alloc`, `kerf_free`, `kerf_call`, `kerf_out_ptr`, `kerf_out_len`.

use std::alloc::{Layout, alloc, dealloc};
use std::sync::Mutex;

static OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn layout(len: usize) -> Layout {
    Layout::from_size_align(len.max(1), 8).expect("valid layout")
}

/// Allocate `len` bytes in linear memory and return the pointer.
#[unsafe(no_mangle)]
pub extern "C" fn kerf_alloc(len: u32) -> u32 {
    // SAFETY: the layout is non-zero sized with valid alignment; the host owns the buffer until kerf_free.
    unsafe { alloc(layout(len as usize)) as usize as u32 }
}

/// Free a buffer obtained from `kerf_alloc` (same `len`).
#[unsafe(no_mangle)]
pub extern "C" fn kerf_free(ptr: u32, len: u32) {
    if ptr == 0 {
        return;
    }
    // SAFETY: ptr/len come from a matching kerf_alloc call made by the host.
    unsafe { dealloc(ptr as usize as *mut u8, layout(len as usize)) }
}

fn read<'a>(ptr: u32, len: u32) -> &'a [u8] {
    if len == 0 {
        return &[];
    }
    // SAFETY: the host wrote `len` bytes at `ptr` (allocated via kerf_alloc) before calling.
    unsafe { std::slice::from_raw_parts(ptr as usize as *const u8, len as usize) }
}

/// Call an engine function by name. Returns 0 on success, 1 on error; either way the output
/// (result or error JSON) is available via `kerf_out_ptr`/`kerf_out_len` until the next call.
#[unsafe(no_mangle)]
pub extern "C" fn kerf_call(fn_ptr: u32, fn_len: u32, in_ptr: u32, in_len: u32) -> i32 {
    let name = String::from_utf8_lossy(read(fn_ptr, fn_len)).into_owned();
    let (code, bytes) = match std::str::from_utf8(read(in_ptr, in_len)) {
        Err(_) => (1, kerf_core::api::err_json("E_PARAM", "input is not valid UTF-8").into_bytes()),
        Ok(input) => match kerf_core::api::call(&name, input) {
            Ok(o) => (0, o.bytes()),
            Err(msg) => (1, kerf_core::api::err_json("E_CALL", &msg).into_bytes()),
        },
    };
    *OUT.lock().unwrap() = bytes;
    code
}

#[unsafe(no_mangle)]
pub extern "C" fn kerf_out_ptr() -> u32 {
    OUT.lock().unwrap().as_ptr() as usize as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn kerf_out_len() -> u32 {
    OUT.lock().unwrap().len() as u32
}
