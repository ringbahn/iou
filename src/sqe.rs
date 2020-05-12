use std::io;
use std::mem;
use std::os::unix::io::RawFd;
use std::ptr::{self, NonNull};
use std::marker::PhantomData;
use std::time::Duration;

use super::IoUring;
use super::{PollFlags, SockAddr, SockFlag};

/// The queue of pending IO events.
///
/// Each element is a [`SubmissionQueueEvent`](crate::sqe::SubmissionQueueEvent).
/// By default, events are processed in parallel after being submitted.
/// You can modify this behavior for specific events using event [`SubmissionFlags`](crate::sqe::SubmissionFlags).
///
/// # Examples
/// Consider a read event that depends on a successful write beforehand.
///
/// We reify this relationship by using `IO_LINK` to link these events.
/// ```rust
/// # use std::error::Error;
/// # use std::fs::File;
/// # use std::os::unix::io::{AsRawFd, RawFd};
/// # use iou::{IoUring, SubmissionFlags};
/// #
/// # fn main() -> Result<(), Box<dyn Error>> {
/// # let mut ring = IoUring::new(2)?;
/// # let mut sq = ring.sq();
/// #
/// let mut write_event = sq.next_sqe().unwrap();
///
/// // -- write event prep elided
///
/// // set IO_LINK to link the next event to this one
/// write_event.set_flags(SubmissionFlags::IO_LINK);
///
/// let mut read_event = sq.next_sqe().unwrap();
///
/// // -- read event prep elided
///
/// // read_event only occurs if write_event was successful
/// sq.submit()?;
/// # Ok(())
/// # }
/// ```
pub struct SubmissionQueue<'ring> {
    ring: NonNull<uring_sys::io_uring>,
    _marker: PhantomData<&'ring mut IoUring>,
}

impl<'ring> SubmissionQueue<'ring> {
    pub(crate) fn new(ring: &'ring IoUring) -> SubmissionQueue<'ring> {
        SubmissionQueue {
            ring: NonNull::from(&ring.ring),
            _marker: PhantomData,
        }
    }

    /// Returns new [`SubmissionQueueEvent`s](crate::sqe::SubmissionQueueEvent) until the queue size is reached. After that, will return `None`.
    /// ```rust
    /// # use iou::IoUring;
    /// # use std::error::Error;
    /// # fn main() -> std::io::Result<()> {
    /// # let ring_size = 2;
    /// let mut ring = IoUring::new(ring_size)?;
    ///
    /// let mut counter = 0;
    ///
    /// while let Some(event) = ring.next_sqe() {
    ///     counter += 1;
    /// }
    ///
    /// assert_eq!(counter, ring_size);
    /// assert!(ring.next_sqe().is_none());
    /// # Ok(())
    /// # }
    ///
    pub fn next_sqe<'a>(&'a mut self) -> Option<SubmissionQueueEvent<'a>> {
        unsafe {
            let sqe = uring_sys::io_uring_get_sqe(self.ring.as_ptr());
            if sqe != ptr::null_mut() {
                let mut sqe = SubmissionQueueEvent::new(&mut *sqe);
                sqe.clear();
                Some(sqe)
            } else {
                None
            }
        }
    }

    /// Submit all events in the queue. Returns the number of submitted events.
    ///
    /// If this function encounters any IO errors an [`io::Error`](std::io::Result) variant is returned.
    pub fn submit(&mut self) -> io::Result<usize> {
        resultify!(unsafe { uring_sys::io_uring_submit(self.ring.as_ptr()) })
    }

    pub fn submit_and_wait(&mut self, wait_for: u32) -> io::Result<usize> {
        resultify!(unsafe { uring_sys::io_uring_submit_and_wait(self.ring.as_ptr(), wait_for as _) })
    }

    pub fn submit_and_wait_with_timeout(&mut self, wait_for: u32, duration: Duration)
        -> io::Result<usize>
    {
        let ts = uring_sys::__kernel_timespec {
            tv_sec: duration.as_secs() as _,
            tv_nsec: duration.subsec_nanos() as _
        };

        loop {
            if let Some(mut sqe) = self.next_sqe() {
                sqe.clear();
                unsafe {
                    sqe.prep_timeout(&ts);
                    sqe.set_user_data(uring_sys::LIBURING_UDATA_TIMEOUT);
                    return resultify!(uring_sys::io_uring_submit_and_wait(self.ring.as_ptr(), wait_for as _))
                }
            }

            self.submit()?;
        }
    }
}

unsafe impl<'ring> Send for SubmissionQueue<'ring> { }
unsafe impl<'ring> Sync for SubmissionQueue<'ring> { }

/// A pending IO event.
///
/// Can be configured with a set of [`SubmissionFlags`](crate::sqe::SubmissionFlags).
///
pub struct SubmissionQueueEvent<'a> {
    sqe: &'a mut uring_sys::io_uring_sqe,
}

impl<'a> SubmissionQueueEvent<'a> {
    pub(crate) fn new(sqe: &'a mut uring_sys::io_uring_sqe) -> SubmissionQueueEvent<'a> {
        SubmissionQueueEvent { sqe }
    }

    /// Get this event's user data.
    pub fn user_data(&self) -> u64 {
        self.sqe.user_data as u64
    }

    /// Set this event's user data. User data is intended to be used by the application after completion.
    /// ```rust
    /// # use iou::IoUring;
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(2)?;
    /// # let mut sq_event = ring.next_sqe().unwrap();
    /// #
    /// sq_event.set_user_data(0xB00);
    /// ring.submit_sqes()?;
    ///
    /// let cq_event = ring.wait_for_cqe()?;
    /// assert_eq!(cq_event.user_data(), 0xB00);
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_user_data(&mut self, user_data: u64) {
        self.sqe.user_data = user_data as _;
    }

    /// Get this event's flags.
    pub fn flags(&self) -> SubmissionFlags {
        unsafe { SubmissionFlags::from_bits_unchecked(self.sqe.flags as _) }
    }

    /// Set this event's flags.
    pub fn set_flags(&mut self, flags: SubmissionFlags) {
        self.sqe.flags = flags.bits() as _;
    }

    #[inline]
    pub unsafe fn prep_read(
        &mut self,
        fd: RawFd,
        buf: &mut [u8],
        offset: usize,
    ) {
        let len = buf.len();
        let addr = buf.as_mut_ptr();
        uring_sys::io_uring_prep_read(self.sqe, fd, addr as _, len as _, offset as _);
    }

    #[inline]
    pub unsafe fn prep_read_vectored(
        &mut self,
        fd: RawFd,
        bufs: &mut [io::IoSliceMut<'_>],
        offset: usize,
    ) {
        let len = bufs.len();
        let addr = bufs.as_mut_ptr();
        uring_sys::io_uring_prep_readv(self.sqe, fd, addr as _, len as _, offset as _);
    }

    #[inline]
    pub unsafe fn prep_read_fixed(
        &mut self,
        fd: RawFd,
        buf: &mut [u8],
        offset: usize,
        buf_index: usize,
    ) {
        let len = buf.len();
        let addr = buf.as_mut_ptr();
        uring_sys::io_uring_prep_read_fixed(self.sqe,
                                      fd,
                                      addr as _,
                                      len as _,
                                      offset as _,
                                      buf_index as _);
    }

    #[inline]
    pub unsafe fn prep_write(
        &mut self,
        fd: RawFd,
        buf: &[u8],
        offset: usize,
    ) {
        let len = buf.len();
        let addr = buf.as_ptr();
        uring_sys::io_uring_prep_write(self.sqe, fd, addr as _, len as _, offset as _);
    }

    #[inline]
    pub unsafe fn prep_write_vectored(
        &mut self,
        fd: RawFd,
        bufs: &[io::IoSlice<'_>],
        offset: usize,
    ) {
        let len = bufs.len();
        let addr = bufs.as_ptr();
        uring_sys::io_uring_prep_writev(self.sqe, fd, addr as _, len as _, offset as _);
    }

    #[inline]
    pub unsafe fn prep_write_fixed(
        &mut self,
        fd: RawFd,
        buf: &[u8],
        offset: usize,
        buf_index: usize,
    ) {
        let len = buf.len();
        let addr = buf.as_ptr();
        uring_sys::io_uring_prep_write_fixed(self.sqe,
                                       fd, addr as _,
                                       len as _,
                                       offset as _,
                                       buf_index as _);
    }

    #[inline]
    pub unsafe fn prep_fsync(&mut self, fd: RawFd, flags: FsyncFlags) {
        uring_sys::io_uring_prep_fsync(self.sqe, fd, flags.bits() as _);
    }

    /// Prepare a timeout event.
    /// ```
    /// # use iou::IoUring;
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(1)?;
    /// # let mut sqe = ring.next_sqe().unwrap();
    /// #
    /// // make a one-second timeout
    /// let timeout_spec: _ = uring_sys::__kernel_timespec {
    ///     tv_sec:  1 as _,
    ///     tv_nsec: 0 as _,
    /// };
    ///
    /// unsafe { sqe.prep_timeout(&timeout_spec); }
    ///
    /// ring.submit_sqes()?;
    /// # Ok(())
    /// # }
    ///```
    #[inline]
    pub unsafe fn prep_timeout(&mut self, ts: &uring_sys::__kernel_timespec) {
        self.prep_timeout_with_flags(ts, 0, TimeoutFlags::empty());
    }

    #[inline]
    pub unsafe fn prep_timeout_with_flags(
        &mut self,
        ts: &uring_sys::__kernel_timespec,
        count: usize,
        flags: TimeoutFlags,
    ) {
        uring_sys::io_uring_prep_timeout(self.sqe,
                                   ts as *const _ as *mut _,
                                   count as _,
                                   flags.bits() as _);
    }

    #[inline]
    pub unsafe fn prep_timeout_remove(&mut self, user_data: u64) {
        uring_sys::io_uring_prep_timeout_remove(self.sqe, user_data as _, 0);
    }

    #[inline]
    pub unsafe fn prep_link_timeout(&mut self, ts: &uring_sys::__kernel_timespec) {
        uring_sys::io_uring_prep_link_timeout(self.sqe, ts as *const _ as *mut _, 0);
    }

    #[inline]
    pub unsafe fn prep_poll_add(&mut self, fd: RawFd, poll_flags: PollFlags) {
        uring_sys::io_uring_prep_poll_add(self.sqe, fd, poll_flags.bits())
    }

    #[inline]
    pub unsafe fn prep_poll_remove(&mut self, user_data: u64) {
        uring_sys::io_uring_prep_poll_remove(self.sqe, user_data as _)
    }

    #[inline]
    pub unsafe fn prep_connect(&mut self, fd: RawFd, socket_addr: &SockAddr) {
        let (addr, len) = socket_addr.as_ffi_pair();
        uring_sys::io_uring_prep_connect(self.sqe, fd, addr as *const _ as *mut _, len);
    }

    #[inline]
    pub unsafe fn prep_accept(&mut self, fd: RawFd, accept: Option<&mut SockAddrStorage>, flags: SockFlag) {
        let (addr, len) = match accept {
            Some(accept) => (accept.storage.as_mut_ptr() as *mut _, &mut accept.len as *mut _ as *mut _),
            None => (std::ptr::null_mut(), std::ptr::null_mut())
        };
        uring_sys::io_uring_prep_accept(self.sqe, fd, addr, len, flags.bits())
    }

    /// Prepare a no-op event.
    /// ```
    /// # use iou::{IoUring, SubmissionFlags};
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(1)?;
    /// #
    /// // example: use a no-op to force a drain
    ///
    /// let mut nop = ring.next_sqe().unwrap();
    ///
    /// nop.set_flags(SubmissionFlags::IO_DRAIN);
    /// unsafe { nop.prep_nop(); }
    ///
    /// ring.submit_sqes()?;
    /// # Ok(())
    /// # }
    ///```
    #[inline]
    pub unsafe fn prep_nop(&mut self) {
        uring_sys::io_uring_prep_nop(self.sqe);
    }

    /// Clear event. Clears user data, flags, and any event setup.
    /// ```
    /// # use iou::{IoUring, SubmissionFlags};
    /// #
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(1)?;
    /// # let mut sqe = ring.next_sqe().unwrap();
    /// #
    /// sqe.set_user_data(0x1010);
    /// sqe.set_flags(SubmissionFlags::IO_DRAIN);
    ///
    /// sqe.clear();
    ///
    /// assert_eq!(sqe.user_data(), 0x0);
    /// assert_eq!(sqe.flags(), SubmissionFlags::empty());
    /// # Ok(())
    /// # }
    /// ```
    pub fn clear(&mut self) {
        *self.sqe = unsafe { mem::zeroed() };
    }

    /// Get a reference to the underlying [`uring_sys::io_uring_sqe`](uring_sys::io_uring_sqe) object.
    ///
    /// You can use this method to inspect the low-level details of an event.
    /// ```
    /// # use iou::{IoUring};
    /// #
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(1)?;
    /// # let mut sqe = ring.next_sqe().unwrap();
    /// #
    /// unsafe { sqe.prep_nop(); }
    ///
    /// let sqe_ref = sqe.raw();
    ///
    /// assert_eq!(sqe_ref.len, 0);
    /// # Ok(())
    /// # }
    ///
    /// ```
    pub fn raw(&self) -> &uring_sys::io_uring_sqe {
        &self.sqe
    }

    pub fn raw_mut(&mut self) -> &mut uring_sys::io_uring_sqe {
        &mut self.sqe
    }
}

unsafe impl<'a> Send for SubmissionQueueEvent<'a> { }
unsafe impl<'a> Sync for SubmissionQueueEvent<'a> { }

pub struct SockAddrStorage {
    storage: mem::MaybeUninit<nix::sys::socket::sockaddr_storage>,
    len: usize,
}

impl SockAddrStorage {
    pub fn uninit() -> Self {
        let storage = mem::MaybeUninit::uninit();
        let len = mem::size_of::<nix::sys::socket::sockaddr_storage>();
        SockAddrStorage {
            storage,
            len
        }
    }

    pub unsafe fn as_socket_addr(&self) -> io::Result<SockAddr> {
        let storage = &*self.storage.as_ptr();
        nix::sys::socket::sockaddr_storage_to_addr(storage, self.len).map_err(|e| {
            let err_no = e.as_errno();
            match err_no {
                Some(err_no) => io::Error::from_raw_os_error(err_no as _),
                None => io::Error::new(io::ErrorKind::Other, "Unknown error")
            }
        })
    }
}

bitflags::bitflags! {
    /// [`SubmissionQueueEvent`](SubmissionQueueEvent) configuration flags.
    ///
    /// Use a [`Registrar`](crate::registrar::Registrar) to register files for the `FIXED_FILE` flag.
    pub struct SubmissionFlags: u8 {
        /// This event's file descriptor is an index into the preregistered set of files.
        const FIXED_FILE    = 1 << 0;   /* use fixed fileset */
        /// Submit this event only after completing all ongoing submission events.
        const IO_DRAIN      = 1 << 1;   /* issue after inflight IO */
        /// Force the next submission event to wait until this event has completed sucessfully.
        ///
        /// An event's link only applies to the next event, but link chains can be
        /// arbitrarily long.
        const IO_LINK       = 1 << 2;   /* next IO depends on this one */
    }
}

bitflags::bitflags! {
    pub struct FsyncFlags: u32 {
        /// Sync file data without an immediate metadata sync.
        const FSYNC_DATASYNC    = 1 << 0;
    }
}

bitflags::bitflags! {
    pub struct TimeoutFlags: u32 {
        const TIMEOUT_ABS   = 1 << 0;
    }
}
