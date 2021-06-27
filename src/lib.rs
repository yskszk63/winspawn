#[allow(unused)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(deref_nullptr)]
#[allow(improper_ctypes)]
mod sys;

mod bindings {
    windows::include_bindings!();
}

use std::ffi::{c_void, OsStr};
use std::future::Future;
use std::io;
use std::iter;
use std::mem;
use std::os::raw::{c_int, c_uint};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::raw::HANDLE;
use std::pin::Pin;
use std::ptr;
use std::task::{Context, Poll, Waker};

use sys::_open_osfhandle;
use sys::_set_thread_local_invalid_parameter_handler;
use sys::wchar_t;
use sys::{_close, _dup, _dup2};
use sys::{_wspawnvp, P_NOWAIT};
use sys::{O_RDONLY, O_WRONLY};

use bindings::Windows::Win32::Foundation::{HANDLE as WinHandle, INVALID_HANDLE_VALUE};
use bindings::Windows::Win32::System::Threading::{
    GetExitCodeProcess, RegisterWaitForSingleObject, UnregisterWaitEx, WaitForSingleObject,
    WAIT_OBJECT_0, WAIT_TIMEOUT, WT_EXECUTEINWAITTHREAD, WT_EXECUTEONLYONCE,
};
use bindings::Windows::Win32::System::WindowsProgramming::INFINITE;

#[cfg(not(windows))]
mod stub {
    use super::*;

    pub type HANDLE = *mut std::ffi::c_void;
    pub(super) fn enc_wstr<S: AsRef<OsStr>>(_: S) -> Vec<wchar_t> {
        panic!("stub")
    }
}
#[cfg(not(windows))]
use stub::*;

#[derive(Debug)]
pub enum Mode {
    ReadOnly,
    ReadWrite,
}

impl Mode {
    fn val(&self) -> c_int {
        match self {
            Self::ReadOnly => O_RDONLY as c_int,
            Self::ReadWrite => O_WRONLY as c_int,
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileDescriptor(c_int);

impl FileDescriptor {
    #[winspawn_macro::ignore_invalid_handler]
    /// doc ok
    pub fn from_raw_handle(handle: HANDLE, mode: Mode) -> io::Result<Self> {
        let r = unsafe { _open_osfhandle(handle as isize, mode.val()) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(r))
    }

    pub unsafe fn from_raw_fd(fd: c_int) -> Self {
        Self(fd)
    }

    #[winspawn_macro::ignore_invalid_handler]
    pub fn dup(&self) -> io::Result<Self> {
        let ret = unsafe { _dup(self.0) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self(ret))
    }

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

pub fn swap_fd<E, R, F>(fd: &FileDescriptor, dest: c_int, func: F) -> Result<R, E>
where
    F: FnOnce(&FileDescriptor) -> Result<R, E>,
    E: From<io::Error>,
{
    let backup = if fd.0 == dest {
        None
    } else {
        log::trace!("begin swap_fd with {:?} {}.", fd, dest);
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
        backup.dup2(dest)?;
    }
    result
}

#[derive(Debug)]
struct Waiter(WinHandle);

impl Drop for Waiter {
    fn drop(&mut self) {
        let ret = unsafe { UnregisterWaitEx(self.0, INVALID_HANDLE_VALUE) };
        if !ret.as_bool() {
            log::warn!("failed to unregister wait: {}", io::Error::last_os_error());
        }
    }
}

#[derive(Debug)]
pub struct Child {
    proc_handle: WinHandle,
    waiter: Option<Waiter>,
}

impl Child {
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
            let mut wait_object = WinHandle::default();
            unsafe {
                RegisterWaitForSingleObject(
                    &mut wait_object as *mut _,
                    this.proc_handle,
                    Some(callback),
                    waker as *mut _,
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

unsafe extern "system" fn callback(ptr: *mut c_void, _: u8) {
    let mut waker = Box::from_raw(ptr as *mut Option<Waker>);
    waker.take().unwrap().wake();
}

#[cfg(windows)]
fn enc_wstr<S: AsRef<OsStr>>(s: S) -> Vec<wchar_t> {
    s.as_ref().encode_wide().chain(iter::once(0)).collect()
}

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
        proc_handle: WinHandle(child),
        waiter: None,
    })
}
