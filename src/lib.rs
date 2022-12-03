#![doc(html_root_url = "https://docs.rs/winspawn/0.1.0")]
//! Spawn process for passing Universal CRT's file descriptor on windows.
//!
//! Using `_spawn` & `_dup`.
//!
//! # Example
//!
//! ```rust
//! use winspawn::{move_fd, spawn, FileDescriptor, Mode};
//!
//! use std::mem;
//! use std::io;
//! use std::fs;
//! use std::os::windows::io::IntoRawHandle;
//!
//! fn main() -> io::Result<()> {
//!     let file = fs::File::open("Cargo.toml")?;
//!     let fd = FileDescriptor::from_raw_handle(file, Mode::ReadOnly)?;
//!
//!     let mut proc = move_fd(&fd, 3, |_| {
//!         // print fd 3 stat
//!         spawn("python", ["-c", r#""import os; print(os.stat(3))""#])
//!     })?;
//!
//!     let exit_code = proc.wait()?;
//!     assert_eq!(0, exit_code);
//!
//!     Ok(())
//! }
//! ```

// download from https://github.com/yskszk63/ucrt-bindings
#[allow(unused)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(deref_nullptr)]
#[allow(improper_ctypes)]
#[allow(non_upper_case_globals)]
mod sys;

use std::ffi::{c_void, OsStr};
use std::future::Future;
use std::io;
use std::iter;
use std::mem;
use std::os::raw::{c_int, c_uint};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::io::IntoRawHandle;
use std::pin::Pin;
use std::ptr;
use std::sync::Once;
use std::task::{Context, Poll, Waker};

use sys::_open_osfhandle;
use sys::_set_thread_local_invalid_parameter_handler;
use sys::wchar_t;
use sys::{_close, _dup, _dup2};
use sys::{_wspawnvp, P_NOWAIT};
use sys::{O_RDONLY, O_RDWR, O_WRONLY};

use windows::Win32::Foundation::{
    BOOLEAN, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Threading::{
    AcquireSRWLockExclusive, GetExitCodeProcess, InitializeSRWLock, RegisterWaitForSingleObject,
    ReleaseSRWLockExclusive, TerminateProcess, UnregisterWaitEx, WaitForSingleObject, RTL_SRWLOCK,
    WT_EXECUTEINWAITTHREAD, WT_EXECUTEONLYONCE,
};
use windows::Win32::System::WindowsProgramming::INFINITE;

/// Open [`FileDescriptor`] mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Read only.
    ReadOnly,
    /// Write only
    WriteOnly,
    /// Read Write
    ReadWrite,
}

impl Mode {
    fn val(&self) -> c_int {
        match self {
            Self::ReadOnly => O_RDONLY as c_int,
            Self::WriteOnly => O_WRONLY as c_int,
            Self::ReadWrite => O_RDWR as c_int,
        }
    }
}

/// Windows File Descriptor (universal CRT).
#[derive(Debug, PartialEq, Eq)]
pub struct FileDescriptor(c_int);

impl FileDescriptor {
    /// Construct FileDescriptor from Windows File Handle.
    #[winspawn_macro::ignore_invalid_handler]
    pub fn from_raw_handle<H>(handle: H, mode: Mode) -> io::Result<Self>
    where
        H: IntoRawHandle,
    {
        let handle = handle.into_raw_handle();
        let r = unsafe { _open_osfhandle(handle as isize, mode.val()) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(r))
    }

    /// Construct FileDescriptor from raw fd.
    ///
    /// # Safety
    /// - Must valid file descriptor
    /// - No other uses this file descriptor
    pub unsafe fn from_raw_fd(fd: c_int) -> Self {
        Self(fd)
    }

    /// Into raw file descriptor.
    pub fn into_raw_fd(self) -> c_int {
        let r = self.0;
        mem::forget(self);
        r
    }

    /// Duplicate File Descriptor. (`_dup`)
    #[winspawn_macro::ignore_invalid_handler]
    pub fn dup(&self) -> io::Result<Self> {
        let ret = unsafe { _dup(self.0) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self(ret))
    }

    /// Duplicate File Descriptor. (`_dup2`)
    #[winspawn_macro::ignore_invalid_handler]
    pub fn dup2(&self, dest: c_int) -> io::Result<Self> {
        let ret = unsafe { _dup2(self.0, dest) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self(dest))
    }
}

impl Drop for FileDescriptor {
    #[winspawn_macro::ignore_invalid_handler]
    fn drop(&mut self) {
        unsafe { _close(self.0) };
    }
}

unsafe fn static_srwlock() -> *mut RTL_SRWLOCK {
    use std::cell::UnsafeCell;

    static mut SWRLOCK: UnsafeCell<RTL_SRWLOCK> = UnsafeCell::new(RTL_SRWLOCK {
        Ptr: ptr::null_mut(),
    });
    static INIT_SRWLOCK: Once = Once::new();

    INIT_SRWLOCK.call_once(|| unsafe {
        InitializeSRWLock(SWRLOCK.get());
    });
    SWRLOCK.get()
}

#[derive(Debug)]
struct StaticMutex(bool);

thread_local!(static ENTERED: std::cell::RefCell<bool> = Default::default());

impl StaticMutex {
    fn acquire() -> Self {
        let enter = ENTERED.with(|b| {
            if *b.borrow() {
                false
            } else {
                *b.borrow_mut() = true;
                true
            }
        });

        if enter {
            unsafe {
                AcquireSRWLockExclusive(static_srwlock());
            }
            Self(true)
        } else {
            Self(false)
        }
    }
}

impl Drop for StaticMutex {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                ENTERED.with(|b| *b.borrow_mut() = false);
                ReleaseSRWLockExclusive(static_srwlock());
            }
        }
    }
}

/// Move fd temporary and call func.
///
/// This function valid in this library lock acquires.
pub fn move_fd<E, R, F>(fd: &FileDescriptor, dest: c_int, func: F) -> Result<R, E>
where
    F: FnOnce(&FileDescriptor) -> Result<R, E>,
    E: From<io::Error>,
{
    log::trace!("begin move_fd with {:?} {}.", fd, dest);

    // lock for modifi file descriptor
    let _ = StaticMutex::acquire();

    let backup = if fd.0 == dest {
        None
    } else {
        // backup dest if exists.
        let original = unsafe { FileDescriptor::from_raw_fd(dest) };
        let backup = original.dup();
        mem::forget(original);
        log::trace!("backup {:?}.", backup);
        backup.ok()
    };

    // drop non inherit flag
    log::trace!("dup. {:?}", fd);
    let dup = fd.dup()?;
    log::trace!("dup2. {:?} {}", dup, dest);
    let newfd = dup.dup2(dest)?;
    drop(dup);
    log::trace!("dup2 ok.");
    let result = func(&newfd);
    drop(newfd);

    // restore backup
    if let Some(backup) = backup {
        log::trace!("restore backup");
        let restored = backup.dup2(dest)?;
        mem::forget(restored);
    }
    result
}

#[derive(Debug)]
struct Waiter(HANDLE);

impl Drop for Waiter {
    fn drop(&mut self) {
        let ret = unsafe { UnregisterWaitEx(self.0, INVALID_HANDLE_VALUE) };
        if !ret.as_bool() {
            log::warn!("failed to unregister wait: {}", io::Error::last_os_error());
        }
    }
}

/// Represent child process.
///
/// An instance is a Future that represents an asynchronous termination.
///
/// # Example
///
/// ```rust
/// use std::io;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> io::Result<()> {
///     let mut proc = winspawn::spawn("cargo", ["--version"])?;
///     let exit_code = proc.await?;
///     assert_eq!(0, exit_code);
///     Ok(())
/// }
/// ```
#[derive(Debug)]
pub struct Child {
    proc_handle: HANDLE,
    waiter: Option<Waiter>,
}

impl Child {
    /// Synchronous wait for exit.
    pub fn wait(&mut self) -> io::Result<u32> {
        let ret = unsafe { WaitForSingleObject(self.proc_handle, INFINITE) };
        if ret != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }

        let mut status = 0;
        unsafe { GetExitCodeProcess(self.proc_handle, &mut status) }
            .ok()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(status)
    }

    /// Try wait for exit.
    ///
    /// Return immediately. If the process is finished, the exit code can be acquired.
    pub fn try_wait(&mut self) -> io::Result<Option<u32>> {
        match unsafe { WaitForSingleObject(self.proc_handle, 0) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => return Ok(None),
            _ => return Err(io::Error::last_os_error()),
        }

        let mut status = 0;
        unsafe { GetExitCodeProcess(self.proc_handle, &mut status) }
            .ok()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(Some(status))
    }

    /// Terminate process.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::io;
    /// use winspawn::spawn;
    ///
    /// fn main() -> io::Result<()> {
    ///     let mut proc = spawn("python", ["-c", r#"import time; time.sleep(0xFFFFFFFF)"#])?;
    ///     proc.kill()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn kill(&mut self) -> io::Result<()> {
        unsafe { TerminateProcess(self.proc_handle, 1) }
            .ok()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

impl Future for Child {
    type Output = io::Result<u32>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = Pin::get_mut(self);

        loop {
            if let Some(..) = &this.waiter {
                if let Some(exitcode) = this.try_wait()? {
                    return Poll::Ready(Ok(exitcode));
                } else {
                    return Poll::Pending;
                }
            }

            if let Some(r) = this.try_wait()? {
                return Poll::Ready(Ok(r));
            }

            let waker = cx.waker().clone();
            let waker = Box::into_raw(Box::new(Some(waker)));
            let mut wait_object = HANDLE::default();
            unsafe {
                RegisterWaitForSingleObject(
                    &mut wait_object as *mut _,
                    this.proc_handle,
                    Some(callback),
                    Some(waker as *const _),
                    INFINITE,
                    WT_EXECUTEINWAITTHREAD | WT_EXECUTEONLYONCE,
                )
            }
            .ok()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            this.waiter = Some(Waiter(wait_object));
        }
    }
}

unsafe extern "system" fn callback(ptr: *mut c_void, _: BOOLEAN) {
    let mut waker = Box::from_raw(ptr as *mut Option<Waker>);
    waker.take().unwrap().wake();
}

#[cfg(windows)]
fn enc_wstr<S: AsRef<OsStr>>(s: S) -> Vec<wchar_t> {
    s.as_ref().encode_wide().chain(iter::once(0)).collect()
}

/// call `_spawnlp`.
///
/// All File Descriptors that do not have the O_NOINHERIT flag will be inherited by the child process.
pub fn spawn<P, A, AS>(program: P, args: A) -> io::Result<Child>
where
    P: AsRef<OsStr>,
    A: IntoIterator<Item = AS>,
    AS: AsRef<OsStr>,
{
    let program = enc_wstr(program.as_ref());
    log::trace!("prog: {:x?}", program);
    let program = program.as_ptr();

    let args = args.into_iter().map(enc_wstr).collect::<Vec<_>>();
    log::trace!("args: {:x?}", args);
    let args = args.iter().map(Vec::as_ptr).collect::<Vec<_>>();

    let args = iter::once(program)
        .chain(args)
        .chain(iter::once(ptr::null()))
        .collect::<Vec<_>>();

    let child = unsafe { _wspawnvp(P_NOWAIT as c_int, program, args.as_ptr()) };
    if child < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(Child {
        proc_handle: HANDLE(child),
        waiter: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutex() {
        let lock1 = StaticMutex::acquire();
        let lock2 = StaticMutex::acquire(); // reentrant
        eprintln!("{:?} {:?}", lock1, lock2);
    }
}
