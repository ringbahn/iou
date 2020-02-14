use std::convert::TryInto;
use std::io;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::os::unix::io::RawFd;

use super::IoUring;

/// A `Registrar` creates ahead-of-time kernel references to files and user buffers.
///
/// Pre-registering kernel IO references greatly reduces per-IO overhead.
/// The kernel no longer needs to obtain and drop file references or map kernel memory for each operation.
/// Consider registering frequently used files and buffers.
/// ```
/// # use iou::{IoUring, Registrar};
/// # fn main() -> std::io::Result<()> {
/// let mut ring = IoUring::new(8)?;
/// let mut registrar: Registrar = ring.registrar();
/// # let fds = &[0, 1];
/// registrar.register_files(fds)?;
/// # Ok(())
/// # }
/// ```
pub struct Registrar<'ring> {
    ring: NonNull<uring_sys::io_uring>,
    fileset: Vec<RegisteredFd>,
    _marker: PhantomData<&'ring mut IoUring>,
}

impl<'ring> Registrar<'ring> {
    pub(crate) fn new(ring: &'ring IoUring) -> Registrar<'ring> {
        Registrar {
            ring: NonNull::from(&ring.ring),
            fileset: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Get the set of currently registered files.
    pub fn fileset(&self) -> &[RegisteredFd] {
        &self.fileset
    }

    /// Register a set of buffers to be mapped into the kernel.
    pub fn register_buffers(&self, buffers: &[io::IoSlice<'_>]) -> io::Result<()> {
        let len = buffers.len();
        let addr = buffers.as_ptr() as *const _;
        let _: i32 = resultify!(unsafe {
            uring_sys::io_uring_register_buffers(self.ring.as_ptr(), addr, len as _)
        })?;
        Ok(())
    }

    /// Unregister all currently registered buffers. An explicit call to this method is often unecessary,
    /// because all buffers will be unregistered automatically when the ring is dropped.
    pub fn unregister_buffers(&self) -> io::Result<()> {
        let _: i32 = resultify!(unsafe {
            uring_sys::io_uring_unregister_buffers(self.ring.as_ptr())
        })?;
        Ok(())
    }

    /// Register a set of files with the kernel. Registered files handle kernel fileset indexing behind the scenes and can often be used in place of raw file descriptors.
    /// ```
    /// # use iou::IoUring;
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(2)?;
    /// # let mut registrar = ring.registrar();
    /// # let raw_fds = [1, 2];
    /// # let bufs = &[std::io::IoSlice::new(b"hi")];
    /// registrar.register_files(&raw_fds)?;
    /// let reg_file = registrar.fileset()[0];
    /// # let mut sqe = ring.next_sqe().unwrap();
    /// unsafe { sqe.prep_write_vectored(reg_file, bufs, 0); }
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_files(&mut self, files: &[RawFd]) -> io::Result<()> {
        let len = files.len();
        let addr = files.as_ptr() as *const _;

        let _: i32 = resultify!(unsafe {
            uring_sys::io_uring_register_files(self.ring.as_ptr(), addr, len as _)
        })?;

        self.fileset = files
            .iter()
            .enumerate()
            .map(|(i, &fd)| RegisteredFd::new(i, fd))
            .collect();

        Ok(())
    }

    /// Update the currently registered kernel fileset. It is usually more efficient to reserve space for files before submitting events, because `IoUring` will wait until the submission queue is
    /// empty before registering files.
    /// # Panics
    /// Panics if `offset` is out of bounds or if the `files` buffer is too large.
    pub fn update_registered_files(&mut self, offset: usize, files: &[RawFd]) -> io::Result<()> {
        if offset + files.len() > self.fileset.len() {
            panic!("attempted out of bounds update for kernel fileset");
        }

        let len = files.len();
        let addr = files.as_ptr() as *const _;

        let _: i32 = resultify!(unsafe {
            uring_sys::io_uring_register_files_update(
                self.ring.as_ptr(),
                offset as _,
                addr,
                len as _,
            )
        })?;

        self.fileset.splice(
            offset..(offset + files.len()),
            (offset..)
                .zip(files.iter())
                .map(|(i, &fd)| RegisteredFd::new(i, fd))
                .collect::<Vec<_>>(),
        );

        Ok(())
    }

    /// Unregister all currently registered files. An explicit call to this method is often unecessary,
    /// because all files will be unregistered automatically when the ring is dropped.
    ///
    /// You can use this method to replace an existing fileset:
    /// ```
    /// # use iou::IoUring;
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(2)?;
    /// # let mut registrar = ring.registrar();
    /// # let raw_fds = [1, 2];
    /// # let bufs = &[std::io::IoSlice::new(b"hi")];
    /// registrar.register_files(&raw_fds)?;
    /// assert!(!registrar.fileset().is_empty());
    ///
    /// registrar.unregister_files()?;
    /// assert!(registrar.fileset().is_empty());
    /// # Ok(())
    /// # }
    /// ```
    pub fn unregister_files(&mut self) -> io::Result<()> {
        let _: i32 =
            resultify!(unsafe { uring_sys::io_uring_unregister_files(self.ring.as_ptr()) })?;
        self.fileset.clear();
        Ok(())
    }

    pub fn register_eventfd(&self, eventfd: RawFd) -> io::Result<()> {
        let _: i32 = resultify!(unsafe {
            uring_sys::io_uring_register_eventfd(self.ring.as_ptr(), eventfd)
        })?;
        Ok(())
    }

    pub fn unregister_eventfd(&self) -> io::Result<()> {
        let _: i32 = resultify!(unsafe {
            uring_sys::io_uring_unregister_eventfd(self.ring.as_ptr())
        })?;
        Ok(())
    }
}

unsafe impl<'ring> Send for Registrar<'ring> { }
unsafe impl<'ring> Sync for Registrar<'ring> { }

/// A member of the kernel's registered fileset.
///
/// Valid `RegisteredFd`s can only be obtained through a [`Registrar`](crate::registrar::Registrar).
///
/// Registered files handle kernel fileset indexing behind the scenes and can often be used in place of raw file descriptors. Not all IO operations support registered files.
///
/// Submission event prep methods on `RegisteredFd` will ensure that the submission event's `SubmissionFlags::FIXED_FILE` flag is properly set.
///
/// # Panics
/// In order to reserve kernel fileset space, `RegisteredFd`s can be placeholders.
/// Placeholders can be interspersed with actual files. Submitted IO events on placeholders will panic.
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

    /// Returns this file's kernel fileset index as a raw file descriptor.
    pub fn as_fd(self) -> RawFd {
        self.index
    }

    /// Check whether this is a placeholder value.
    pub fn is_placeholder(self) -> bool {
        self.index == -1
    }
}

/// IoUring file handles.
#[derive(Debug, Copy, Clone)]
pub enum RingFd {
    /// A raw file descriptor.
    Raw(RawFd),
    /// A member of the kernel's fixed fileset.
    Registered(RegisteredFd),
}

impl RingFd {
    pub fn raw(self) -> RawFd {
        match self {
            RingFd::Raw(fd) => fd,
            RingFd::Registered(index) => index.as_fd(),
        }
    }
}

impl From<RawFd> for RingFd {
    fn from(item: RawFd) -> RingFd {
        RingFd::Raw(item)
    }
}

impl From<RegisteredFd> for RingFd {
    fn from(item: RegisteredFd) -> RingFd {
        if item.is_placeholder() {
            panic!("attempted to perform IO on kernel fileset placeholder");
        }
        RingFd::Registered(item)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

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
    #[should_panic]
    // fails with device busy
    fn double_register() {
        let ring = IoUring::new(8).unwrap();
        ring.registrar().register_files(&[1]).unwrap();
        ring.registrar().register_files(&[1]).unwrap();
    }

    #[test]
    #[should_panic(expected = "attempted out of bounds update for kernel fileset")]
    fn out_of_bounds_update() {
        let raw_fds = [1, 2];
        let ring = IoUring::new(8).unwrap();
        ring.registrar().register_files(&raw_fds).unwrap();
        ring.registrar().unregister_files().unwrap();
        ring.registrar()
            .update_registered_files(0, &raw_fds)
            .unwrap();
    }
}
