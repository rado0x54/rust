//! System bindings for the BPF platform
//!
//! This module contains the facade (aka platform-specific) implementations of
//! OS level functionality for BPF
//!
//! This is all super highly experimental and not actually intended for
//! wide/production use yet, it's still all in the experimental category. This
//! will likely change over time.
//!
//! Currently all functions here are basically stubs that immediately return
//! errors. The hope is that with a portability lint we can turn actually just
//! remove all this and just omit parts of the standard library if we're
//! compiling for BPF. That way it's a compile time error for something that's
//! guaranteed to be a runtime error!

use crate::os::raw::c_char;

pub mod alloc;
pub mod args;
//#[cfg(feature = "backtrace")]
//pub mod backtrace;
pub mod cmath;
pub mod env;
pub mod fs;
pub mod io;
pub mod memchr;
pub mod net;
pub mod os;
pub mod path;
pub mod pipe;
pub mod process;
pub mod thread;
pub mod time;
pub mod stdio;

pub mod condvar;
pub mod mutex;
pub mod rwlock;
pub mod thread_local_dtor;
pub mod thread_local_key;

pub use crate::sys_common::os_str_bytes as os_str;

extern "C" {
    fn abort() -> !;
    #[allow(improper_ctypes)]
    fn custom_panic(info: &core::panic::PanicInfo<'_>);
    fn sol_log_(message: *const u8, length: u64);
}

pub fn sol_log(message: &str) {
    unsafe {
        sol_log_(message.as_ptr(), message.len() as u64);
    }
}

pub fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe { custom_panic(info); }
    unsafe { abort(); }
}

pub fn unsupported<T>() -> crate::io::Result<T> {
    Err(unsupported_err())
}

pub fn unsupported_err() -> crate::io::Error {
    crate::io::Error::new(crate::io::ErrorKind::Other,
                   "operation not supported on BPF yet")
}

pub fn decode_error_kind(_code: i32) -> crate::io::ErrorKind {
    crate::io::ErrorKind::Other
}

// This enum is used as the storage for a bunch of types which can't actually
// exist.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Void {}

pub unsafe fn strlen(mut s: *const c_char) -> usize {
    let mut n = 0;
    while *s != 0 {
        n += 1;
        s = s.offset(1);
    }
    return n
}

pub fn abort_internal() -> ! {
    unsafe { abort() }
}

// We don't have randomness yet, but I totally used a random number generator to
// generate these numbers.
//
// More seriously though this is just for DOS protection in hash maps. It's ok
// if we don't do that on BPF just yet.
pub fn hashmap_random_keys() -> (u64, u64) {
    (1, 2)
}
