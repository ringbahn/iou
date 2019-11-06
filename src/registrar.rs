use std::io;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::os::unix::io::RawFd;

use super::{IoUring, sys};

pub struct Registrar<'ring> {
    ring: NonNull<sys::io_uring>,
    _marker: PhantomData<&'ring mut IoUring>,
}

impl<'ring> Registrar<'ring> {
    pub(crate) fn new(ring: &'ring IoUring) -> Registrar<'ring> {
        Registrar {
            ring: NonNull::from(&ring.ring),
            _marker: PhantomData,
        }
    }
    pub fn register_buffers(&self, buffers: &[io::IoSlice]) -> io::Result<()> {
        let res = unsafe {
            let len = buffers.len();
            let addr = buffers as *const [io::IoSlice] as *const libc::iovec;
            sys::io_uring_register_buffers(self.ring.as_ptr(), addr, len as _)
        };

        if res >= 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(res))
        }
    }

    pub fn unregister_buffers(&self) -> io::Result<()> {
        let res = unsafe {
            sys::io_uring_unregister_buffers(self.ring.as_ptr())
        };

        if res >= 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(res))
        }
    }

    pub fn register_files(&self, files: &[RawFd]) -> io::Result<()> {
        let res = unsafe {
            let len = files.len();
            let addr = files as *const [RawFd] as *const RawFd;
            sys::io_uring_register_files(self.ring.as_ptr(), addr, len as _)
        };

        if res >= 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(res))
        }
    }

    pub fn unregister_files(&self) -> io::Result<()> {
        let res = unsafe {
            sys::io_uring_unregister_files(self.ring.as_ptr())
        };

        if res >= 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(res))
        }
    }

    pub fn register_eventfd(&self, eventfd: RawFd) -> io::Result<()> {
        let res = unsafe {
            sys::io_uring_register_eventfd(self.ring.as_ptr(), eventfd)
        };

        if res >= 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(res))
        }
    }

    pub fn unregister_eventfd(&self) -> io::Result<()> {
        let res = unsafe {
            sys::io_uring_unregister_eventfd(self.ring.as_ptr())
        };

        if res >= 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(res))
        }
    }
}
