use std::convert::TryInto;
use std::io;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::os::unix::io::{AsRawFd, RawFd};

use crate::{IoUring, Probe, SQE, resultify};

/// A `Registrar` creates ahead-of-time kernel references to files and user buffers.
///
/// Preregistration significantly reduces per-IO overhead, so consider registering frequently
/// used files and buffers. For file IO, preregistration lets the kernel skip the atomic acquire and
/// release of a kernel-specific file descriptor. For buffer IO, the kernel can avoid mapping kernel
/// memory for every operation.
///
/// Beware that registration is relatively expensive and should be done before any performance
/// sensitive code.
///
/// If you want to register a file but don't have an open file descriptor yet, you can register
/// a [placeholder](crate::RegisteredFd::placeholder) descriptor and
/// [update](crate::registrar::Registrar::update_registered_files) it later.
/// ```
/// # use iou::{IoUring, Registrar, RegisteredFd};
/// # fn main() -> std::io::Result<()> {
/// let mut ring = IoUring::new(8)?;
/// let mut registrar: Registrar = ring.registrar();
/// # let fds = &[0, 1];
/// let registered_files: Vec<RegisteredFd> = registrar.register_files(fds)?.collect();
/// # Ok(())
/// # }
/// ```
pub struct Registrar<'ring> {
    ring: NonNull<uring_sys::io_uring>,
    _marker: PhantomData<&'ring mut IoUring>,
}

impl<'ring> Registrar<'ring> {
    pub(crate) fn new(ring: &'ring IoUring) -> Registrar<'ring> {
        Registrar {
            ring: NonNull::from(&ring.ring),
            _marker: PhantomData,
        }
    }

    /// Register a set of buffers to be mapped into the kernel.
    pub fn register_buffers(&self, buffers: &[io::IoSlice<'_>]) -> io::Result<()> {
        let len = buffers.len();
        let addr = buffers.as_ptr() as *const _;
        resultify(unsafe {
            uring_sys::io_uring_register_buffers(self.ring.as_ptr(), addr, len as _)
        })?;
        Ok(())
    }

    /// Unregister all currently registered buffers. An explicit call to this method is often unecessary,
    /// because all buffers will be unregistered automatically when the ring is dropped.
    pub fn unregister_buffers(&self) -> io::Result<()> {
        resultify(unsafe {
            uring_sys::io_uring_unregister_buffers(self.ring.as_ptr())
        })?;
        Ok(())
    }

    /// Register a set of files with the kernel. Registered files handle kernel fileset indexing 
    /// behind the scenes and can often be used in place of raw file descriptors.
    /// 
    /// # Errors
    /// Returns an error if
    /// * there is a preexisting set of registered files,
    /// * the `files` slice was empty,
    /// * the inner [`io_uring_register_files`](uring_sys::io_uring_register_files) call failed for
    ///   another reason
    /// ```no_run
    /// # use iou::IoUring;
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(2)?;
    /// # let mut registrar = ring.registrar();
    /// # let raw_fds = [1, 2];
    /// # let bufs = &[std::io::IoSlice::new(b"hi")];
    /// let fileset: Vec<_> = registrar.register_files(&raw_fds)?.collect();
    /// let reg_file = fileset[0];
    /// # let mut sqe = ring.next_sqe().unwrap();
    /// unsafe { sqe.prep_write_vectored(reg_file, bufs, 0); }
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_files<'a>(&mut self, files: &'a [RawFd]) -> io::Result<impl Iterator<Item = RegisteredFd> + 'a> {
        resultify(unsafe {
            uring_sys::io_uring_register_files(
                self.ring.as_ptr(), 
                files.as_ptr() as *const _, 
                files.len() as _
            )
        })?;
        Ok(files
            .iter()
            .enumerate()
            .map(|(i, &fd)| RegisteredFd::new(i, fd))
        )
    }

    /// Update the currently registered kernel fileset. It is usually more efficient to reserve space
    /// for files before submitting events, because `IoUring` will wait until the submission queue is
    /// empty before registering files.
    /// # Errors
    /// Returns an error if
    /// * there isn't a registered fileset,
    /// * the `files` slice was empty,
    /// * `offset` is out of bounds, 
    /// * the `files` slice was too large,
    /// * the inner [`io_uring_register_files_update`](uring_sys::io_uring_register_files_update) call
    ///   failed for another reason
    pub fn update_registered_files<'a>(&mut self, offset: usize, files: &'a [RawFd]) -> io::Result<impl Iterator<Item = RegisteredFd> + 'a> {
        resultify(unsafe {
            uring_sys::io_uring_register_files_update(
                self.ring.as_ptr(),
                offset as _,
                files.as_ptr() as *const _,
                files.len() as _,
            )
        })?;
        Ok(files
            .iter()
            .enumerate()
            .map(move |(i, &fd)| RegisteredFd::new(i + offset, fd))
        )
    }

    /// Unregister all currently registered files. An explicit call to this method is often unecessary,
    /// because all files will be unregistered automatically when the ring is dropped.
    ///
    /// # Errors
    /// Returns an error if
    /// * there isn't a registered fileset,
    /// * the inner [`io_uring_unregister_files`](uring_sys::io_uring_unregister_files) call
    /// failed for another reason
    ///
    /// You can use this method to replace an existing fileset:
    /// ```
    /// # use iou::IoUring;
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(2)?;
    /// # let mut registrar = ring.registrar();
    /// let raw_fds = [0, 1];
    /// let fds: Vec<_> = registrar.register_files(&raw_fds)?.collect();
    /// assert_eq!(fds.len(), 2);
    ///
    /// registrar.unregister_files()?;
    ///
    /// let other_raw_fds = [0, 1, 2];
    /// let new_fds: Vec<_> = registrar.register_files(&other_raw_fds)?.collect();
    /// assert_eq!(new_fds.len(), 3);
    /// # Ok(())
    /// # }
    /// ```
    pub fn unregister_files(&mut self) -> io::Result<()> {
        resultify(unsafe { uring_sys::io_uring_unregister_files(self.ring.as_ptr()) })?;
        Ok(())
    }

    pub fn register_eventfd(&self, eventfd: RawFd) -> io::Result<()> {
        resultify(unsafe {
            uring_sys::io_uring_register_eventfd(self.ring.as_ptr(), eventfd)
        })?;
        Ok(())
    }

    pub fn register_eventfd_async(&self, eventfd: RawFd) -> io::Result<()> {
        resultify(unsafe {
            uring_sys::io_uring_register_eventfd_async(self.ring.as_ptr(), eventfd)
        })?;
        Ok(())
    }

    pub fn unregister_eventfd(&self) -> io::Result<()> {
        resultify(unsafe {
            uring_sys::io_uring_unregister_eventfd(self.ring.as_ptr())
        })?;
        Ok(())
    }

    pub fn register_personality(&self) -> io::Result<Personality> {
        let id = resultify(unsafe { uring_sys::io_uring_register_personality(self.ring.as_ptr()) })?;
        debug_assert!(id < u16::MAX as u32);
        Ok(Personality { id: id as u16 })
    }

    pub fn unregister_personality(&self, personality: Personality) -> io::Result<()> {
        resultify(unsafe {
            uring_sys::io_uring_unregister_personality(self.ring.as_ptr(), personality.id as _)
        })?;
        Ok(())
    }

    pub fn probe(&self) -> io::Result<Probe> {
        Probe::for_ring(self.ring.as_ptr())
    }
}

unsafe impl<'ring> Send for Registrar<'ring> { }
unsafe impl<'ring> Sync for Registrar<'ring> { }

/// A member of the kernel's registered fileset.
///
/// Valid `RegisteredFd`s can only be obtained through a [`Registrar`](crate::registrar::Registrar).
///
/// Registered files handle kernel fileset indexing behind the scenes and can often be used in place
/// of raw file descriptors. Not all IO operations support registered files.
///
/// Submission event prep methods on `RegisteredFd` will ensure that the submission event's
/// `SubmissionFlags::FIXED_FILE` flag is properly set.
///
/// # Panics
/// In order to reserve kernel fileset space, `RegisteredFd`s can be placeholders.
/// Placeholders can be interspersed with actual files. Attempted IO events on placeholders will panic.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct RegisteredFd {
    index: RawFd,
}

impl RegisteredFd {
    pub(crate) fn new(index: usize, fd: RawFd) -> RegisteredFd {
        if fd == -1 {
            RegisteredFd::placeholder()
        } else {
            RegisteredFd {
                index: index.try_into().unwrap(),
            }
        }
    }

    /// Get a new `RegisteredFd` placeholder. Used to reserve kernel fileset entries.
    pub fn placeholder() -> RegisteredFd {
        RegisteredFd { index: -1 }
    }

    /// Returns this file's kernel fileset index.
    /// ```
    /// # use iou::RegisteredFd;
    /// let ph = RegisteredFd::placeholder();
    /// assert_eq!(ph.index(), None);
    /// ```
    pub fn index(self) -> Option<u32> {
        if self.is_placeholder() {
            None
        } else {
            Some(self.index as u32)
        }
    }

    /// Check whether this is a placeholder value.
    pub fn is_placeholder(self) -> bool {
        self.index == -1
    }
}

pub trait RingFd {
    fn as_raw_fd(&self) -> RawFd;
    fn set_flags(&self, sqe: &mut SQE<'_>);
}

impl RingFd for RawFd {
    fn as_raw_fd(&self) -> RawFd {
        *self
    }
    fn set_flags(&self, _: &mut SQE<'_>) { }
}

impl AsRawFd for RegisteredFd {
    fn as_raw_fd(&self) -> RawFd {
        if self.is_placeholder() {
            panic!("attempted to perform IO on kernel fileset placeholder");
        } else {
            self.index
        }
    }

}

impl RingFd for RegisteredFd {
    fn as_raw_fd(&self) -> RawFd {
        AsRawFd::as_raw_fd(self)
    }
    fn set_flags(&self, sqe: &mut SQE<'_>) {
        sqe.set_fixed_file();
    }
}

#[derive(Eq, PartialEq, Hash, Ord, PartialOrd, Clone, Copy)]
pub struct Personality {
    pub(crate) id: u16,
}

impl From<u16> for Personality {
    fn from(id: u16) -> Personality {
        Personality { id }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::os::unix::io::AsRawFd;

    #[test]
    #[should_panic(expected = "Invalid argument")]
    fn register_empty_slice() {
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().register_files(&[]).unwrap();
    }

    #[test]
    #[should_panic(expected = "Bad file descriptor")]
    fn register_bad_fd() {
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().register_files(&[-100]).unwrap();
    }

    #[test]
    #[should_panic(expected = "attempted to perform IO on kernel fileset placeholder")]
    fn placeholder_submit() {
        let mut ring = IoUring::new(1).unwrap();
        let mut sqe = ring.next_sqe().unwrap();

        unsafe {
            sqe.prep_read_vectored(RegisteredFd::placeholder(), &mut [], 0);
        }
    }

    #[test]
    #[should_panic(expected = "Device or resource busy")]
    fn double_register() {
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().register_files(&[1]).unwrap();
        let _ = ring.registrar().register_files(&[1]).unwrap();
    }

    #[test]
    #[should_panic(expected = "No such device or address")]
    fn empty_unregister_err() {
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().unregister_files().unwrap();
    }

    #[test]
    #[should_panic(expected = "No such device or address")]
    fn empty_update_err() {
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().update_registered_files(0, &[1]).unwrap();
    }

    #[test]
    #[should_panic(expected = "Invalid argument")]
    fn offset_out_of_bounds_update() {
        let raw_fds = [1, 2];
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().register_files(&raw_fds).unwrap();
        let _ = ring.registrar().update_registered_files(2, &raw_fds).unwrap();
    }

    #[test]
    #[should_panic(expected = "Invalid argument")]
    fn slice_len_out_of_bounds_update() {
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().register_files(&[1, 1]).unwrap();
        let _ = ring.registrar().update_registered_files(0, &[1, 1, 1]).unwrap();
    }

    #[test]
    fn valid_fd_update() {
        let ring = IoUring::new(1).unwrap();

        let file = std::fs::File::create("tmp.txt").unwrap();
        let _ = ring.registrar().register_files(&[file.as_raw_fd()]).unwrap();

        let new_file = std::fs::File::create("new_tmp.txt").unwrap();
        let _ = ring.registrar().update_registered_files(0, &[new_file.as_raw_fd()]).unwrap();

        let _ = std::fs::remove_file("tmp.txt");
        let _ = std::fs::remove_file("new_tmp.txt");
    }

    #[test]
    fn placeholder_update() {
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().register_files(&[-1, -1, -1]).unwrap();

        let file = std::fs::File::create("tmp.txt").unwrap();
        let _ = ring.registrar().update_registered_files(0, &[file.as_raw_fd()]).unwrap();
        let _ = std::fs::remove_file("tmp.txt");
    }
}
