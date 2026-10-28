//! Verification root for the verified modules of kerf-core: Verus checks these files exactly as
//! kerf-core compiles them (`engines/rust/verus/verify.sh`). math and num come along as unverified
//! context (compiled, not verified); nothing else of kerf-core is part of this crate.
#![no_std]
#![cfg_attr(verus_keep_ghost, feature(proc_macro_hygiene))]
extern crate alloc;

#[path = "../kerf-core/src/math.rs"]
pub mod math;
#[path = "../kerf-core/src/num.rs"]
pub mod num;
#[path = "../kerf-core/src/pdf/writer.rs"]
pub mod writer;
#[path = "../kerf-core/src/png.rs"]
pub mod png;
#[path = "../kerf-core/src/sort.rs"]
pub mod sort;
#[path = "../kerf-core/src/raster/kernels.rs"]
pub mod kernels;
#[path = "../kerf-core/src/json.rs"]
pub mod json;
