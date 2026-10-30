//! The only place with `unsafe`: thin, safe wrappers over the OS (process groups / job objects, shutdown
//! signals, the random source, file locks). One implementation per platform behind the same function names.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
