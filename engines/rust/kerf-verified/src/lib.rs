//! Kerf kernels proven with Verus: see engines/rust/verus/VERUS.md for what is proven and the
//! trusted base. Plain stable Rust with no dependencies: specifications and proofs live in the same
//! files but are compiled only under `--cfg verus_keep_ghost` (engines/rust/verus/verify.sh).
#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(verus_keep_ghost, feature(proc_macro_hygiene))]
extern crate alloc;

pub mod dec;
pub mod json;
pub mod pdf;
pub mod png;
pub mod raster;
pub mod sort;
