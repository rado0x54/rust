#![cfg_attr(test, allow(unused))]

#[cfg(test)]
mod tests;

use crate::io::prelude::*;

#[cfg(not(target_arch = "bpf"))]
use crate::cell::{Cell, RefCell};
use crate::fmt;
#[cfg(not(target_arch = "bpf"))]
use crate::io::{self, BufReader, Initializer, IoSlice, IoSliceMut, LineWriter};
#[cfg(target_arch = "bpf")]
use crate::io::{self, BufReader, Initializer, IoSlice, IoSliceMut};
#[cfg(not(target_arch = "bpf"))]
use crate::lazy::SyncOnceCell;
#[cfg(not(target_arch = "bpf"))]
use crate::pin::Pin;
#[cfg(not(target_arch = "bpf"))]
use crate::sync::atomic::{AtomicBool, Ordering};
use crate::sync::{Arc, Mutex, MutexGuard};
use crate::sys::stdio;
#[cfg(not(target_arch = "bpf"))]
use crate::sys_common::remutex::{ReentrantMutex, ReentrantMutexGuard};

type LocalStream = Arc<Mutex<Vec<u8>>>;

#[cfg(not(target_arch = "bpf"))]
thread_local! {
    /// Used by the test crate to capture the output of the print macros and panics.
    static OUTPUT_CAPTURE: Cell<Option<LocalStream>> = {
        Cell::new(None)
    }
}

/// Flag to indicate OUTPUT_CAPTURE is used.
///
/// If it is None and was never set on any thread, this flag is set to false,
/// and OUTPUT_CAPTURE can be safely ignored on all threads, saving some time
/// and memory registering an unused thread local.
///
/// Note about memory ordering: This contains information about whether a
/// thread local variable might be in use. Although this is a global flag, the
/// memory ordering between threads does not matter: we only want this flag to
/// have a consistent order between set_output_capture and print_to *within
/// the same thread*. Within the same thread, things always have a perfectly
/// consistent order. So Ordering::Relaxed is fine.
#[cfg(not(target_arch = "bpf"))]
static OUTPUT_CAPTURE_USED: AtomicBool = AtomicBool::new(false);

/// A handle to a raw instance of the standard input stream of this process.
///
/// This handle is not synchronized or buffered in any fashion. Constructed via
/// the `std::io::stdio::stdin_raw` function.
struct StdinRaw(stdio::Stdin);

/// A handle to a raw instance of the standard output stream of this process.
///
/// This handle is not synchronized or buffered in any fashion. Constructed via
/// the `std::io::stdio::stdout_raw` function.
struct StdoutRaw(stdio::Stdout);

/// A handle to a raw instance of the standard output stream of this process.
///
/// This handle is not synchronized or buffered in any fashion. Constructed via
/// the `std::io::stdio::stderr_raw` function.
struct StderrRaw(stdio::Stderr);

/// Constructs a new raw handle to the standard input of this process.
///
/// The returned handle does not interact with any other handles created nor
/// handles returned by `std::io::stdin`. Data buffered by the `std::io::stdin`
/// handles is **not** available to raw handles returned from this function.
///
/// The returned handle has no external synchronization or buffering.
#[unstable(feature = "libstd_sys_internals", issue = "none")]
#[cfg(not(target_arch = "bpf"))]
const fn stdin_raw() -> StdinRaw {
    StdinRaw(stdio::Stdin::new())
}

/// Constructs a new raw handle to the standard output stream of this process.
///
/// The returned handle does not interact with any other handles created nor
/// handles returned by `std::io::stdout`. Note that data is buffered by the
/// `std::io::stdout` handles so writes which happen via this raw handle may
/// appear before previous writes.
///
/// The returned handle has no external synchronization or buffering layered on
/// top.
#[unstable(feature = "libstd_sys_internals", issue = "none")]
#[cfg(not(target_arch = "bpf"))]
const fn stdout_raw() -> StdoutRaw {
    StdoutRaw(stdio::Stdout::new())
}

/// Constructs a new raw handle to the standard error stream of this process.
///
/// The returned handle does not interact with any other handles created nor
/// handles returned by `std::io::stderr`.
///
/// The returned handle has no external synchronization or buffering layered on
/// top.
#[unstable(feature = "libstd_sys_internals", issue = "none")]
#[cfg(not(target_arch = "bpf"))]
const fn stderr_raw() -> StderrRaw {
    StderrRaw(stdio::Stderr::new())
}

impl Read for StdinRaw {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        handle_ebadf(self.0.read(buf), 0)
    }

    fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        handle_ebadf(self.0.read_vectored(bufs), 0)
    }

    #[inline]
    fn is_read_vectored(&self) -> bool {
        self.0.is_read_vectored()
    }

    #[inline]
    unsafe fn initializer(&self) -> Initializer {
        Initializer::nop()
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        handle_ebadf(self.0.read_to_end(buf), 0)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        handle_ebadf(self.0.read_to_string(buf), 0)
    }
}

impl Write for StdoutRaw {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        handle_ebadf(self.0.write(buf), buf.len())
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        let total = bufs.iter().map(|b| b.len()).sum();
        handle_ebadf(self.0.write_vectored(bufs), total)
    }

    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }

    fn flush(&mut self) -> io::Result<()> {
        handle_ebadf(self.0.flush(), ())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        handle_ebadf(self.0.write_all(buf), ())
    }

    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        handle_ebadf(self.0.write_all_vectored(bufs), ())
    }

    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        handle_ebadf(self.0.write_fmt(fmt), ())
    }
}

impl Write for StderrRaw {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        handle_ebadf(self.0.write(buf), buf.len())
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        let total = bufs.iter().map(|b| b.len()).sum();
        handle_ebadf(self.0.write_vectored(bufs), total)
    }

    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }

    fn flush(&mut self) -> io::Result<()> {
        handle_ebadf(self.0.flush(), ())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        handle_ebadf(self.0.write_all(buf), ())
    }

    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        handle_ebadf(self.0.write_all_vectored(bufs), ())
    }

    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        handle_ebadf(self.0.write_fmt(fmt), ())
    }
}

fn handle_ebadf<T>(r: io::Result<T>, default: T) -> io::Result<T> {
    match r {
        Err(ref e) if stdio::is_ebadf(e) => Ok(default),
        r => r,
    }
}

/// A handle to the standard input stream of a process.
///
/// Each handle is a shared reference to a global buffer of input data to this
/// process. A handle can be `lock`'d to gain full access to [`BufRead`] methods
/// (e.g., `.lines()`). Reads to this handle are otherwise locked with respect
/// to other reads.
///
/// This handle implements the `Read` trait, but beware that concurrent reads
/// of `Stdin` must be executed with care.
///
/// Created by the [`io::stdin`] method.
///
/// [`io::stdin`]: stdin
///
/// ### Note: Windows Portability Consideration
///
/// When operating in a console, the Windows implementation of this stream does not support
/// non-UTF-8 byte sequences. Attempting to read bytes that are not valid UTF-8 will return
/// an error.
///
/// # Examples
///
/// ```no_run
/// use std::io::{self, Read};
///
/// fn main() -> io::Result<()> {
///     let mut buffer = String::new();
///     let mut stdin = io::stdin(); // We get `Stdin` here.
///     stdin.read_to_string(&mut buffer)?;
///     Ok(())
/// }
/// ```
#[stable(feature = "rust1", since = "1.0.0")]
pub struct Stdin {
    #[cfg(not(target_arch = "bpf"))]
    inner: &'static Mutex<BufReader<StdinRaw>>,
}

/// A locked reference to the [`Stdin`] handle.
///
/// This handle implements both the [`Read`] and [`BufRead`] traits, and
/// is constructed via the [`Stdin::lock`] method.
///
/// ### Note: Windows Portability Consideration
///
/// When operating in a console, the Windows implementation of this stream does not support
/// non-UTF-8 byte sequences. Attempting to read bytes that are not valid UTF-8 will return
/// an error.
///
/// # Examples
///
/// ```no_run
/// use std::io::{self, Read};
///
/// fn main() -> io::Result<()> {
///     let mut buffer = String::new();
///     let stdin = io::stdin(); // We get `Stdin` here.
///     {
///         let mut handle = stdin.lock(); // We get `StdinLock` here.
///         handle.read_to_string(&mut buffer)?;
///     } // `StdinLock` is dropped here.
///     Ok(())
/// }
/// ```
#[stable(feature = "rust1", since = "1.0.0")]
pub struct StdinLock<'a> {
    inner: MutexGuard<'a, BufReader<StdinRaw>>,
}

/// Constructs a new handle to the standard input of the current process.
///
/// Each handle returned is a reference to a shared global buffer whose access
/// is synchronized via a mutex. If you need more explicit control over
/// locking, see the [`Stdin::lock`] method.
///
/// ### Note: Windows Portability Consideration
/// When operating in a console, the Windows implementation of this stream does not support
/// non-UTF-8 byte sequences. Attempting to read bytes that are not valid UTF-8 will return
/// an error.
///
/// # Examples
///
/// Using implicit synchronization:
///
/// ```no_run
/// use std::io::{self, Read};
///
/// fn main() -> io::Result<()> {
///     let mut buffer = String::new();
///     io::stdin().read_to_string(&mut buffer)?;
///     Ok(())
/// }
/// ```
///
/// Using explicit synchronization:
///
/// ```no_run
/// use std::io::{self, Read};
///
/// fn main() -> io::Result<()> {
///     let mut buffer = String::new();
///     let stdin = io::stdin();
///     let mut handle = stdin.lock();
///
///     handle.read_to_string(&mut buffer)?;
///     Ok(())
/// }
/// ```
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
pub fn stdin() -> Stdin {
    static INSTANCE: SyncOnceCell<Mutex<BufReader<StdinRaw>>> = SyncOnceCell::new();
    Stdin {
        inner: INSTANCE.get_or_init(|| {
            Mutex::new(BufReader::with_capacity(stdio::STDIN_BUF_SIZE, stdin_raw()))
        }),
    }
}

/// BPF dummy
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(target_arch = "bpf")]
pub fn stdin() -> Stdin {
    Stdin {}
}

impl Stdin {
    /// Locks this handle to the standard input stream, returning a readable
    /// guard.
    ///
    /// The lock is released when the returned lock goes out of scope. The
    /// returned guard also implements the [`Read`] and [`BufRead`] traits for
    /// accessing the underlying data.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io::{self, Read};
    ///
    /// fn main() -> io::Result<()> {
    ///     let mut buffer = String::new();
    ///     let stdin = io::stdin();
    ///     let mut handle = stdin.lock();
    ///
    ///     handle.read_to_string(&mut buffer)?;
    ///     Ok(())
    /// }
    /// ```
    #[stable(feature = "rust1", since = "1.0.0")]
    #[cfg(not(target_arch = "bpf"))]
    pub fn lock(&self) -> StdinLock<'_> {
        StdinLock { inner: self.inner.lock().unwrap_or_else(|e| e.into_inner()) }
    }

    /// Locks this handle and reads a line of input, appending it to the specified buffer.
    ///
    /// For detailed semantics of this method, see the documentation on
    /// [`BufRead::read_line`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// let mut input = String::new();
    /// match io::stdin().read_line(&mut input) {
    ///     Ok(n) => {
    ///         println!("{} bytes read", n);
    ///         println!("{}", input);
    ///     }
    ///     Err(error) => println!("error: {}", error),
    /// }
    /// ```
    ///
    /// You can run the example one of two ways:
    ///
    /// - Pipe some text to it, e.g., `printf foo | path/to/executable`
    /// - Give it text interactively by running the executable directly,
    ///   in which case it will wait for the Enter key to be pressed before
    ///   continuing
    #[stable(feature = "rust1", since = "1.0.0")]
    #[cfg(not(target_arch = "bpf"))]
    pub fn read_line(&self, buf: &mut String) -> io::Result<usize> {
        self.lock().read_line(buf)
    }
}

#[stable(feature = "std_debug", since = "1.16.0")]
impl fmt::Debug for Stdin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stdin").finish_non_exhaustive()
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.lock().read(buf)
    }
    fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        self.lock().read_vectored(bufs)
    }
    #[inline]
    fn is_read_vectored(&self) -> bool {
        self.lock().is_read_vectored()
    }
    #[inline]
    unsafe fn initializer(&self) -> Initializer {
        Initializer::nop()
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.lock().read_to_end(buf)
    }
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.lock().read_to_string(buf)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.lock().read_exact(buf)
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(target_arch = "bpf")]
impl Read for Stdin {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
    fn read_vectored(&mut self, _bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        Ok(0)
    }
    #[inline]
    fn is_read_vectored(&self) -> bool {
        false
    }
    #[inline]
    unsafe fn initializer(&self) -> Initializer {
        Initializer::nop()
    }
    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Ok(0)
    }
    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Ok(0)
    }
    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Ok(())
    }
}

// only used by platform-dependent io::copy specializations, i.e. unused on some platforms
#[cfg(any(target_os = "linux", target_os = "android"))]
impl StdinLock<'_> {
    pub(crate) fn as_mut_buf(&mut self) -> &mut BufReader<impl Read> {
        &mut self.inner
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl Read for StdinLock<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        self.inner.read_vectored(bufs)
    }

    #[inline]
    fn is_read_vectored(&self) -> bool {
        self.inner.is_read_vectored()
    }

    #[inline]
    unsafe fn initializer(&self) -> Initializer {
        Initializer::nop()
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.inner.read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.inner.read_to_string(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.inner.read_exact(buf)
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl BufRead for StdinLock<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, n: usize) {
        self.inner.consume(n)
    }

    fn read_until(&mut self, byte: u8, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.inner.read_until(byte, buf)
    }

    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        self.inner.read_line(buf)
    }
}

#[stable(feature = "std_debug", since = "1.16.0")]
impl fmt::Debug for StdinLock<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdinLock").finish_non_exhaustive()
    }
}

/// A handle to the global standard output stream of the current process.
///
/// Each handle shares a global buffer of data to be written to the standard
/// output stream. Access is also synchronized via a lock and explicit control
/// over locking is available via the [`lock`] method.
///
/// Created by the [`io::stdout`] method.
///
/// ### Note: Windows Portability Consideration
/// When operating in a console, the Windows implementation of this stream does not support
/// non-UTF-8 byte sequences. Attempting to write bytes that are not valid UTF-8 will return
/// an error.
///
/// [`lock`]: Stdout::lock
/// [`io::stdout`]: stdout
#[stable(feature = "rust1", since = "1.0.0")]
pub struct Stdout {
    // FIXME: this should be LineWriter or BufWriter depending on the state of
    //        stdout (tty or not). Note that if this is not line buffered it
    //        should also flush-on-panic or some form of flush-on-abort.
    #[cfg(not(target_arch = "bpf"))]
    inner: Pin<&'static ReentrantMutex<RefCell<LineWriter<StdoutRaw>>>>,
}

/// A locked reference to the [`Stdout`] handle.
///
/// This handle implements the [`Write`] trait, and is constructed via
/// the [`Stdout::lock`] method. See its documentation for more.
///
/// ### Note: Windows Portability Consideration
/// When operating in a console, the Windows implementation of this stream does not support
/// non-UTF-8 byte sequences. Attempting to write bytes that are not valid UTF-8 will return
/// an error.
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
pub struct StdoutLock<'a> {
    inner: ReentrantMutexGuard<'a, RefCell<LineWriter<StdoutRaw>>>,
}

/// BPF dummy
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(target_arch = "bpf")]
pub struct StdoutLock {
}

#[cfg(not(target_arch = "bpf"))]
static STDOUT: SyncOnceCell<ReentrantMutex<RefCell<LineWriter<StdoutRaw>>>> = SyncOnceCell::new();

/// Constructs a new handle to the standard output of the current process.
///
/// Each handle returned is a reference to a shared global buffer whose access
/// is synchronized via a mutex. If you need more explicit control over
/// locking, see the [`Stdout::lock`] method.
///
/// ### Note: Windows Portability Consideration
/// When operating in a console, the Windows implementation of this stream does not support
/// non-UTF-8 byte sequences. Attempting to write bytes that are not valid UTF-8 will return
/// an error.
///
/// # Examples
///
/// Using implicit synchronization:
///
/// ```no_run
/// use std::io::{self, Write};
///
/// fn main() -> io::Result<()> {
///     io::stdout().write_all(b"hello world")?;
///
///     Ok(())
/// }
/// ```
///
/// Using explicit synchronization:
///
/// ```no_run
/// use std::io::{self, Write};
///
/// fn main() -> io::Result<()> {
///     let stdout = io::stdout();
///     let mut handle = stdout.lock();
///
///     handle.write_all(b"hello world")?;
///
///     Ok(())
/// }
/// ```
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
pub fn stdout() -> Stdout {
    Stdout {
        inner: Pin::static_ref(&STDOUT).get_or_init_pin(
            || unsafe { ReentrantMutex::new(RefCell::new(LineWriter::new(stdout_raw()))) },
            |mutex| unsafe { mutex.init() },
        ),
    }
}

/// Dummy stdout for BPF target
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(target_arch = "bpf")]
pub fn stdout() -> Stdout {
    Stdout {}
}

#[cfg(not(target_arch = "bpf"))]
pub fn cleanup() {
    if let Some(instance) = STDOUT.get() {
        // Flush the data and disable buffering during shutdown
        // by replacing the line writer by one with zero
        // buffering capacity.
        // We use try_lock() instead of lock(), because someone
        // might have leaked a StdoutLock, which would
        // otherwise cause a deadlock here.
        if let Some(lock) = Pin::static_ref(instance).try_lock() {
            *lock.borrow_mut() = LineWriter::with_capacity(0, stdout_raw());
        }
    }
}

impl Stdout {
    /// Locks this handle to the standard output stream, returning a writable
    /// guard.
    ///
    /// The lock is released when the returned lock goes out of scope. The
    /// returned guard also implements the `Write` trait for writing data.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io::{self, Write};
    ///
    /// fn main() -> io::Result<()> {
    ///     let stdout = io::stdout();
    ///     let mut handle = stdout.lock();
    ///
    ///     handle.write_all(b"hello world")?;
    ///
    ///     Ok(())
    /// }
    /// ```
    #[stable(feature = "rust1", since = "1.0.0")]
    #[cfg(not(target_arch = "bpf"))]
    pub fn lock(&self) -> StdoutLock<'_> {
        StdoutLock {
            inner: self.inner.lock()
        }
    }
}

#[stable(feature = "std_debug", since = "1.16.0")]
impl fmt::Debug for Stdout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stdout").finish_non_exhaustive()
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self).write(buf)
    }
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        (&*self).write_vectored(bufs)
    }
    #[inline]
    fn is_write_vectored(&self) -> bool {
        io::Write::is_write_vectored(&&*self)
    }
    fn flush(&mut self) -> io::Result<()> {
        (&*self).flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        (&*self).write_all(buf)
    }
    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        (&*self).write_all_vectored(bufs)
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> io::Result<()> {
        (&*self).write_fmt(args)
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(target_arch = "bpf")]
impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unsafe {
            crate::sys::sol_log(core::str::from_utf8_unchecked(buf));
        }
        Ok(buf.len())
    }
    fn write_vectored(&mut self, _bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        Ok(0)
    }
    #[inline]
    fn is_write_vectored(&self) -> bool {
        false
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        unsafe {
            crate::sys::sol_log(core::str::from_utf8_unchecked(buf));
        }
        Ok(())
    }
    fn write_all_vectored(&mut self, _bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        Ok(())
    }
    fn write_fmt(&mut self, _args: fmt::Arguments<'_>) -> io::Result<()> {
        Ok(())
    }
}

#[stable(feature = "write_mt", since = "1.48.0")]
#[cfg(not(target_arch = "bpf"))]
impl Write for &Stdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.lock().write(buf)
    }
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.lock().write_vectored(bufs)
    }
    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.lock().is_write_vectored()
    }
    fn flush(&mut self) -> io::Result<()> {
        self.lock().flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.lock().write_all(buf)
    }
    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        self.lock().write_all_vectored(bufs)
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> io::Result<()> {
        self.lock().write_fmt(args)
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
impl Write for StdoutLock<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.borrow_mut().write(buf)
    }
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.inner.borrow_mut().write_vectored(bufs)
    }
    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.inner.borrow_mut().is_write_vectored()
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.borrow_mut().flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.borrow_mut().write_all(buf)
    }
    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        self.inner.borrow_mut().write_all_vectored(bufs)
    }
}

#[stable(feature = "std_debug", since = "1.16.0")]
#[cfg(not(target_arch = "bpf"))]
impl fmt::Debug for StdoutLock<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdoutLock").finish_non_exhaustive()
    }
}

#[stable(feature = "std_debug", since = "1.16.0")]
#[cfg(target_arch = "bpf")]
impl fmt::Debug for StdoutLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("StdoutLock { .. }")
    }
}

/// A handle to the standard error stream of a process.
///
/// For more information, see the [`io::stderr`] method.
///
/// [`io::stderr`]: stderr
///
/// ### Note: Windows Portability Consideration
/// When operating in a console, the Windows implementation of this stream does not support
/// non-UTF-8 byte sequences. Attempting to write bytes that are not valid UTF-8 will return
/// an error.
#[stable(feature = "rust1", since = "1.0.0")]
pub struct Stderr {
    #[cfg(not(target_arch = "bpf"))]
    inner: Pin<&'static ReentrantMutex<RefCell<StderrRaw>>>,
}

/// A locked reference to the [`Stderr`] handle.
///
/// This handle implements the [`Write`] trait and is constructed via
/// the [`Stderr::lock`] method. See its documentation for more.
///
/// ### Note: Windows Portability Consideration
/// When operating in a console, the Windows implementation of this stream does not support
/// non-UTF-8 byte sequences. Attempting to write bytes that are not valid UTF-8 will return
/// an error.
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
pub struct StderrLock<'a> {
    inner: ReentrantMutexGuard<'a, RefCell<StderrRaw>>,
}

/// BPF dummy
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(target_arch = "bpf")]
pub struct StderrLock {
}

/// Constructs a new handle to the standard error of the current process.
///
/// This handle is not buffered.
///
/// ### Note: Windows Portability Consideration
/// When operating in a console, the Windows implementation of this stream does not support
/// non-UTF-8 byte sequences. Attempting to write bytes that are not valid UTF-8 will return
/// an error.
///
/// # Examples
///
/// Using implicit synchronization:
///
/// ```no_run
/// use std::io::{self, Write};
///
/// fn main() -> io::Result<()> {
///     io::stderr().write_all(b"hello world")?;
///
///     Ok(())
/// }
/// ```
///
/// Using explicit synchronization:
///
/// ```no_run
/// use std::io::{self, Write};
///
/// fn main() -> io::Result<()> {
///     let stderr = io::stderr();
///     let mut handle = stderr.lock();
///
///     handle.write_all(b"hello world")?;
///
///     Ok(())
/// }
/// ```
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
pub fn stderr() -> Stderr {
    // Note that unlike `stdout()` we don't use `at_exit` here to register a
    // destructor. Stderr is not buffered , so there's no need to run a
    // destructor for flushing the buffer
    static INSTANCE: SyncOnceCell<ReentrantMutex<RefCell<StderrRaw>>> = SyncOnceCell::new();

    Stderr {
        inner: Pin::static_ref(&INSTANCE).get_or_init_pin(
            || unsafe { ReentrantMutex::new(RefCell::new(stderr_raw())) },
            |mutex| unsafe { mutex.init() },
        ),
    }
}

/// BPF dummy
#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(target_arch = "bpf")]
pub fn stderr() -> Stderr {
    Stderr {}
}

impl Stderr {
    /// Locks this handle to the standard error stream, returning a writable
    /// guard.
    ///
    /// The lock is released when the returned lock goes out of scope. The
    /// returned guard also implements the [`Write`] trait for writing data.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::{self, Write};
    ///
    /// fn foo() -> io::Result<()> {
    ///     let stderr = io::stderr();
    ///     let mut handle = stderr.lock();
    ///
    ///     handle.write_all(b"hello world")?;
    ///
    ///     Ok(())
    /// }
    /// ```
    #[stable(feature = "rust1", since = "1.0.0")]
    #[cfg(not(target_arch = "bpf"))]
    pub fn lock(&self) -> StderrLock<'_> {
        StderrLock { inner: self.inner.lock() }
    }
}

#[stable(feature = "std_debug", since = "1.16.0")]
impl fmt::Debug for Stderr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stderr").finish_non_exhaustive()
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self).write(buf)
    }
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        (&*self).write_vectored(bufs)
    }
    #[inline]
    fn is_write_vectored(&self) -> bool {
        io::Write::is_write_vectored(&&*self)
    }
    fn flush(&mut self) -> io::Result<()> {
        (&*self).flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        (&*self).write_all(buf)
    }
    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        (&*self).write_all_vectored(bufs)
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> io::Result<()> {
        (&*self).write_fmt(args)
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(target_arch = "bpf")]
impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unsafe {
            crate::sys::sol_log(core::str::from_utf8_unchecked(buf));
        }
        Ok(buf.len())
    }
    fn write_vectored(&mut self, _bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        Ok(0)
    }
    #[inline]
    fn is_write_vectored(&self) -> bool {
        false
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        unsafe {
            crate::sys::sol_log(core::str::from_utf8_unchecked(buf));
        }
        Ok(())
    }
    fn write_all_vectored(&mut self, _bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        Ok(())
    }
    fn write_fmt(&mut self, _args: fmt::Arguments<'_>) -> io::Result<()> {
        Ok(())
    }
}

#[stable(feature = "write_mt", since = "1.48.0")]
#[cfg(not(target_arch = "bpf"))]
impl Write for &Stderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.lock().write(buf)
    }
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.lock().write_vectored(bufs)
    }
    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.lock().is_write_vectored()
    }
    fn flush(&mut self) -> io::Result<()> {
        self.lock().flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.lock().write_all(buf)
    }
    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        self.lock().write_all_vectored(bufs)
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> io::Result<()> {
        self.lock().write_fmt(args)
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[cfg(not(target_arch = "bpf"))]
impl Write for StderrLock<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.borrow_mut().write(buf)
    }
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.inner.borrow_mut().write_vectored(bufs)
    }
    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.inner.borrow_mut().is_write_vectored()
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.borrow_mut().flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.borrow_mut().write_all(buf)
    }
    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        self.inner.borrow_mut().write_all_vectored(bufs)
    }
}

#[stable(feature = "std_debug", since = "1.16.0")]
#[cfg(not(target_arch = "bpf"))]
impl fmt::Debug for StderrLock<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StderrLock").finish_non_exhaustive()
    }
}

#[stable(feature = "std_debug", since = "1.16.0")]
#[cfg(target_arch = "bpf")]
impl fmt::Debug for StderrLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("StderrLock { .. }")
    }
}

/// Sets the thread-local output capture buffer and returns the old one.
#[cfg(not(target_arch = "bpf"))]
#[unstable(
    feature = "internal_output_capture",
    reason = "this function is meant for use in the test crate \
        and may disappear in the future",
    issue = "none"
)]
#[doc(hidden)]
pub fn set_output_capture(sink: Option<LocalStream>) -> Option<LocalStream> {
    if sink.is_none() && !OUTPUT_CAPTURE_USED.load(Ordering::Relaxed) {
        // OUTPUT_CAPTURE is definitely None since OUTPUT_CAPTURE_USED is false.
        return None;
    }
    OUTPUT_CAPTURE_USED.store(true, Ordering::Relaxed);
    OUTPUT_CAPTURE.with(move |slot| slot.replace(sink))
}

/// Dummy version for satisfying test library dependencies when building the BPF target.
#[cfg(target_arch = "bpf")]
#[unstable(
    feature = "internal_output_capture",
    reason = "this function is meant for use in the test crate \
        and may disappear in the future",
    issue = "none"
)]
#[doc(hidden)]
pub fn set_output_capture(_sink: Option<LocalStream>) -> Option<LocalStream> {
    None
}

/// Write `args` to the capture buffer if enabled and possible, or `global_s`
/// otherwise. `label` identifies the stream in a panic message.
///
/// This function is used to print error messages, so it takes extra
/// care to avoid causing a panic when `local_s` is unusable.
/// For instance, if the TLS key for the local stream is
/// already destroyed, or if the local stream is locked by another
/// thread, it will just fall back to the global stream.
///
/// However, if the actual I/O causes an error, this function does panic.
#[cfg(not(target_arch = "bpf"))]
fn print_to<T>(args: fmt::Arguments<'_>, global_s: fn() -> T, label: &str)
where
    T: Write,
{
    if OUTPUT_CAPTURE_USED.load(Ordering::Relaxed)
        && OUTPUT_CAPTURE.try_with(|s| {
            // Note that we completely remove a local sink to write to in case
            // our printing recursively panics/prints, so the recursive
            // panic/print goes to the global sink instead of our local sink.
            s.take().map(|w| {
                let _ = w.lock().unwrap_or_else(|e| e.into_inner()).write_fmt(args);
                s.set(Some(w));
            })
        }) == Ok(Some(()))
    {
        // Succesfully wrote to capture buffer.
        return;
    }

    if let Err(e) = global_s().write_fmt(args) {
        panic!("failed printing to {}: {}", label, e);
    }
}

#[unstable(
    feature = "print_internals",
    reason = "implementation detail which may disappear or be replaced at any time",
    issue = "none"
)]
#[doc(hidden)]
#[cfg(not(test))]
#[cfg(not(target_arch = "bpf"))]
pub fn _print(args: fmt::Arguments<'_>) {
    print_to(args, stdout, "stdout");
}

#[unstable(
    feature = "print_internals",
    reason = "implementation detail which may disappear or be replaced at any time",
    issue = "none")]
#[doc(hidden)]
#[cfg(not(test))]
#[cfg(target_arch = "bpf")]
pub fn _print(_args: fmt::Arguments<'_>) {
}

#[unstable(
    feature = "print_internals",
    reason = "implementation detail which may disappear or be replaced at any time",
    issue = "none"
)]
#[doc(hidden)]
#[cfg(not(test))]
#[cfg(not(target_arch = "bpf"))]
pub fn _eprint(args: fmt::Arguments<'_>) {
    print_to(args, stderr, "stderr");
}

#[unstable(
    feature = "print_internals",
    reason = "implementation detail which may disappear or be replaced at any time",
    issue = "none")]
#[doc(hidden)]
#[cfg(not(test))]
#[cfg(target_arch = "bpf")]
pub fn _eprint(_args: fmt::Arguments<'_>) {
}

#[cfg(test)]
pub use realstd::io::{_eprint, _print};
