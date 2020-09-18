use std::convert::TryInto;
use std::io;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::os::unix::io::RawFd;

use super::IoUring;

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
    num_reg_files: Option<NonZeroU32>,
    _marker: PhantomData<&'ring mut IoUring>,
}

impl<'ring> Registrar<'ring> {
    pub(crate) fn new(ring: &'ring IoUring) -> Registrar<'ring> {
        Registrar {
            ring: NonNull::from(&ring.ring),
            num_reg_files: None,
            _marker: PhantomData,
        }
    }

    /// Get the number of currently registered files, if any. This method returns
    /// `None` if there aren't any registered files.
    pub fn fileset_size(&self) -> Option<u32> {
        self.num_reg_files.map(|num| num.get())
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

    /// Register a set of files with the kernel. Registered files handle kernel fileset indexing 
    /// behind the scenes and can often be used in place of raw file descriptors.
    /// 
    /// # Errors
    /// Returns an error if
    /// * there is a preexisting set of registered files, 
    /// * an empty file descriptor slice is passed in, 
    /// * the inner [`io_uring_register_files`](uring_sys::io_uring_register_files) call failed
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
        if files.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty `files` slice"));
        } else if self.num_reg_files.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Other, 
                "there is a preexisting registered fileset"
            ));
        }
        
        let _: i32 = resultify!(unsafe {
            uring_sys::io_uring_register_files(
                self.ring.as_ptr(), 
                files.as_ptr() as *const _, 
                files.len() as _
            )
        })?;
        
        self.num_reg_files = Some(NonZeroU32::new(files.len() as u32).unwrap());
        Ok(files
            .iter()
            .enumerate()
            .map(|(i, &fd)| RegisteredFd::new(i, fd))
        )
    }

    /// Update the currently registered kernel fileset. It is usually more efficient to reserve space for files before submitting events, because `IoUring` will wait until the submission queue is
    /// empty before registering files.
    /// # Errors
    /// Returns an error if
    /// * there isn't a registered fileset,
    /// * `offset` is out of bounds, 
    /// * the `files` buffer is too large, 
    /// * the inner [`io_uring_register_files_update`](uring_sys::io_uring_register_files_update) call failed
    pub fn update_registered_files<'a>(&mut self, offset: usize, files: &'a [RawFd]) -> io::Result<impl Iterator<Item = RegisteredFd> + 'a> {
        if self.fileset_size().is_none() { 
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "no fileset to update"
            ));

        } else if offset + files.len() > self.fileset_size().unwrap().try_into().unwrap() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "attempted out of bounds update for registered fileset"
            ));
        }

        let _: i32 = resultify!(unsafe {
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
    /// * the inner [`io_uring_unregister_files`](uring_sys::io_uring_unregister_files) call failed
    ///
    /// You can use this method to replace an existing fileset:
    /// ```
    /// # use iou::IoUring;
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(2)?;
    /// # let mut registrar = ring.registrar();
    /// let raw_fds = [0, 1];
    /// let fds: Vec<_> = registrar.register_files(&raw_fds)?.collect();
    /// assert_eq!(registrar.fileset_size(), Some(2));
    ///
    /// registrar.unregister_files()?;
    /// assert!(registrar.fileset_size().is_none());
    ///
    /// let other_raw_fds = [0, 1, 2];
    /// let new_fds: Vec<_> = registrar.register_files(&other_raw_fds)?.collect();
    /// assert_eq!(registrar.fileset_size(), Some(3));
    /// # Ok(())
    /// # }
    /// ```
    pub fn unregister_files(&mut self) -> io::Result<()> {
        if self.num_reg_files.is_none() {
            return Err(io::Error::new(io::ErrorKind::Other, "no fileset to unregister"));
        }
        let _: i32 =
            resultify!(unsafe { uring_sys::io_uring_unregister_files(self.ring.as_ptr()) })?;
        self.num_reg_files = None;
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

    /// Returns this file's kernel fileset index as a raw file descriptor.
    /// ```
    /// # use iou::RegisteredFd;
    /// let ph = RegisteredFd::placeholder();
    /// assert_eq!(ph.as_fd(), -1);
    /// ```
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
    #[should_panic(expected = "empty `files` slice")]
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
    #[should_panic(expected = "there is a preexisting registered fileset")]
    fn double_register() {
        let ring = IoUring::new(1).unwrap();
        let mut regis = ring.registrar();
        let _ = regis.register_files(&[1]).unwrap();
        let _ = regis.register_files(&[1]).unwrap();
    }

    #[test]
    #[should_panic(expected = "no fileset to unregister")]
    fn empty_unregister_err() {
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().unregister_files().unwrap();
    }

    #[test]
    #[should_panic(expected = "no fileset to update")]
    fn empty_update_err() {
        let ring = IoUring::new(1).unwrap();
        let _ = ring.registrar().update_registered_files(0, &[1]).unwrap();
    }

    #[test]
    #[should_panic(expected = "attempted out of bounds update for registered fileset")]
    fn offset_out_of_bounds_update() {
        let raw_fds = [1, 2];
        let ring = IoUring::new(1).unwrap();
        let mut regis = ring.registrar();
        let _ = regis.register_files(&raw_fds).unwrap();
        let _ = regis.update_registered_files(2, &raw_fds).unwrap();
    }

    #[test]
    #[should_panic(expected = "attempted out of bounds update for registered fileset")]
    fn slice_len_out_of_bounds_update() {
        let ring = IoUring::new(1).unwrap();
        let mut regis = ring.registrar();
        let _ = regis.register_files(&[1, 1]).unwrap();
        let _ = regis.update_registered_files(0, &[1, 1, 1]).unwrap();
    }

    #[test]
    fn valid_fd_update() {
        let ring = IoUring::new(1).unwrap();
        let mut regis = ring.registrar();
        let _ = regis.register_files(&[1, 2, 1]).unwrap();
        let _ = regis.update_registered_files(0, &[2, 1, 2]).unwrap();
    }

    #[test]
    fn placeholder_update() {
        let ring = IoUring::new(1).unwrap();
        let mut regis = ring.registrar();
        let _ = regis.register_files(&[-1, -1, -1]).unwrap();
        let _ = regis.update_registered_files(0, &[0, 1, 2]).unwrap();
    }
}
