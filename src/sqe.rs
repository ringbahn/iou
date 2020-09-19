use std::io;
use std::mem;
use std::ffi::CStr;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::RawFd;
use std::ptr;
use std::slice;

use super::RingFd;

pub use nix::fcntl::{OFlag, FallocateFlags, PosixFadviseAdvice};
pub use nix::poll::PollFlags;
pub use nix::sys::epoll::{EpollOp, EpollEvent};
pub use nix::sys::mman::MmapAdvise;
pub use nix::sys::stat::Mode;
pub use nix::sys::socket::{SockAddr, SockFlag, MsgFlags};

use crate::Personality;

/// A pending IO event.
///
/// Can be configured with a set of [`SubmissionFlags`](crate::sqe::SubmissionFlags).
///
pub struct SQE<'a> {
    sqe: &'a mut uring_sys::io_uring_sqe,
}

impl<'a> SQE<'a> {
    pub(crate) fn new(sqe: &'a mut uring_sys::io_uring_sqe) -> SQE<'a> {
        SQE { sqe }
    }

    /// Get this event's user data.
    pub fn user_data(&self) -> u64 {
        self.sqe.user_data as u64
    }

    /// Set this event's user data. User data is intended to be used by the application after completion.
    ///
    /// # Safety
    ///
    /// This function is marked `unsafe`. The library from which you obtained this
    /// `SQE` may impose additional safety invariants which you must adhere to
    /// when setting the user_data for a submission queue event, which it may rely on when
    /// processing the corresponding completion queue event. For example, the library
    /// [ringbahn][ringbahn] 
    ///
    /// # Example
    ///
    /// ```rust
    /// # use iou::IoUring;
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(2)?;
    /// # let mut sq_event = ring.prepare_sqe().unwrap();
    /// #
    /// unsafe { sq_event.set_user_data(0xB00); }
    /// ring.submit_sqes()?;
    ///
    /// let cq_event = ring.wait_for_cqe()?;
    /// assert_eq!(cq_event.user_data(), 0xB00);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [ringbahn]: https://crates.io/crates/ringbahn
    pub unsafe fn set_user_data(&mut self, user_data: u64) {
        self.sqe.user_data = user_data as _;
    }

    /// Get this event's flags.
    pub fn flags(&self) -> SubmissionFlags {
        unsafe { SubmissionFlags::from_bits_unchecked(self.sqe.flags as _) }
    }

    /// Overwrite this event's flags.
    pub fn overwrite_flags(&mut self, flags: SubmissionFlags) {
        self.sqe.flags = flags.bits() as _;
    }

    // must be called after any prep methods to properly complete mapped kernel IO
    #[inline]
    fn set_fixed_file(&mut self) {
        self.set_flags(self.flags() | SubmissionFlags::FIXED_FILE);
    }

    /// Set these flags for this event (any flags already set will still be set).
    pub fn set_flags(&mut self, flags: SubmissionFlags) {
        self.sqe.flags &= flags.bits();
    }

    pub fn set_personality(&mut self, personality: Personality) {
        self.sqe.buf_index.buf_index.personality = personality.id;
    }

    #[inline]
    pub unsafe fn prep_read(
        &mut self,
        fd: RawFd,
        buf: &mut [u8],
        offset: u64,
    ) {
        let len = buf.len();
        let addr = buf.as_mut_ptr();
        uring_sys::io_uring_prep_read(self.sqe, fd, addr as _, len as _, offset as _);
    }

    #[inline]
    pub unsafe fn prep_read_vectored(
        &mut self,
        fd: impl Into<RingFd>,
        bufs: &mut [io::IoSliceMut<'_>],
        offset: u64,
    ) {
        let fd = fd.into();
        let len = bufs.len();
        let addr = bufs.as_mut_ptr();
        uring_sys::io_uring_prep_readv(self.sqe, fd.raw(), addr as _, len as _, offset as _);
        if let RingFd::Registered(_) = fd { self.set_fixed_file(); };
    }

    #[inline]
    pub unsafe fn prep_read_fixed(
        &mut self,
        fd: impl Into<RingFd>,
        buf: &mut [u8],
        offset: u64,
        buf_index: u32,
    ) {
        let fd = fd.into();
        let len = buf.len();
        let addr = buf.as_mut_ptr();
        uring_sys::io_uring_prep_read_fixed(self.sqe,
                                      fd.raw(),
                                      addr as _,
                                      len as _,
                                      offset as _,
                                      buf_index as _);
        if let RingFd::Registered(_) = fd { self.set_fixed_file(); };
    }

    #[inline]
    pub unsafe fn prep_write(
        &mut self,
        fd: RawFd,
        buf: &[u8],
        offset: u64,
    ) {
        let len = buf.len();
        let addr = buf.as_ptr();
        uring_sys::io_uring_prep_write(self.sqe, fd, addr as _, len as _, offset as _);
    }

    #[inline]
    pub unsafe fn prep_write_vectored(
        &mut self,
        fd: impl Into<RingFd>,
        bufs: &[io::IoSlice<'_>],
        offset: u64,
    ) {
        let fd = fd.into();
        let len = bufs.len();
        let addr = bufs.as_ptr();
        uring_sys::io_uring_prep_writev(self.sqe,
                                    fd.raw(),
                                    addr as _,
                                    len as _,
                                    offset as _);
        if let RingFd::Registered(_) = fd { self.set_fixed_file(); };
    }

    #[inline]
    pub unsafe fn prep_write_fixed(
        &mut self,
        fd: impl Into<RingFd>,
        buf: &[u8],
        offset: u64,
        buf_index: usize,
    ) {
        let fd = fd.into();
        let len = buf.len();
        let addr = buf.as_ptr();
        uring_sys::io_uring_prep_write_fixed(self.sqe,
                                       fd.raw(),
                                       addr as _,
                                       len as _,
                                       offset as _,
                                       buf_index as _);
        if let RingFd::Registered(_) = fd { self.set_fixed_file(); };
    }

    #[inline]
    pub unsafe fn prep_fsync(&mut self, fd: impl Into<RingFd>, flags: FsyncFlags) {
        let fd = fd.into();
        uring_sys::io_uring_prep_fsync(self.sqe, fd.raw(), flags.bits() as _);
        if let RingFd::Registered(_) = fd { self.set_fixed_file(); };
    }

    pub unsafe fn prep_splice(
        &mut self,
        fd_in: RawFd,
        off_in: i64,
        fd_out: RawFd,
        off_out: i64,
        count: u32,
        flags: SpliceFlags,
    ) {
        uring_sys::io_uring_prep_splice(self.sqe, fd_in, off_in, fd_out, off_out, count, flags.bits());
    }

    #[inline]
    pub unsafe fn prep_recv(&mut self, fd: RawFd, buf: &mut [u8], flags: MsgFlags) {
        let data = buf.as_mut_ptr() as *mut libc::c_void;
        let len = buf.len();
        uring_sys::io_uring_prep_send(self.sqe, fd, data, len, flags.bits());
    }

    #[inline]
    pub unsafe fn prep_send(&mut self, fd: RawFd, buf: &[u8], flags: MsgFlags) {
        let data = buf.as_ptr() as *const libc::c_void as *mut libc::c_void;
        let len = buf.len();
        uring_sys::io_uring_prep_send(self.sqe, fd, data, len, flags.bits());
    }

    // TODO sendmsg and recvmsg
    //
    #[inline]
    pub unsafe fn prep_fallocate(&mut self, fd: RawFd,
                                 offset: u64, size: u64,
                                 flags: FallocateFlags) {
        uring_sys::io_uring_prep_fallocate(self.sqe, fd,
                                        flags.bits() as _,
                                        offset as _,
                                        size as _);
    }

    #[inline]
    pub unsafe fn prep_statx(
        &mut self,
        dirfd: RawFd,
        path: &CStr,
        flags: StatxFlags,
        mask: StatxMode,
        buf: &mut libc::statx,
    ) {
        uring_sys::io_uring_prep_statx(self.sqe, dirfd, path.as_ptr() as _,
                                       flags.bits() as _, mask.bits() as _,
                                       buf as _);
    }

    #[inline]
    pub unsafe fn prep_openat(
        &mut self,
        fd: RawFd,
        path: &CStr,
        flags: OFlag,
        mode: Mode,
    ) {
        uring_sys::io_uring_prep_openat(self.sqe, fd, path.as_ptr() as _, flags.bits(), mode.bits());
    }

    // TODO openat2

    #[inline]
    pub unsafe fn prep_close(&mut self, fd: RawFd) {
        uring_sys::io_uring_prep_close(self.sqe, fd);
    }


    /// Prepare a timeout event.
    /// ```
    /// # use iou::IoUring;
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(1)?;
    /// # let mut sqe = ring.prepare_sqe().unwrap();
    /// #
    /// // make a one-second timeout
    /// let timeout_spec: _ = uring_sys::__kernel_timespec {
    ///     tv_sec:  1 as _,
    ///     tv_nsec: 0 as _,
    /// };
    ///
    /// unsafe { sqe.prep_timeout(&timeout_spec, 0); }
    ///
    /// ring.submit_sqes()?;
    /// # Ok(())
    /// # }
    ///```
    #[inline]
    pub unsafe fn prep_timeout(&mut self, ts: &uring_sys::__kernel_timespec, events: u32) {
        self.prep_timeout_with_flags(ts, events, TimeoutFlags::empty());
    }

    #[inline]
    pub unsafe fn prep_timeout_with_flags(
        &mut self,
        ts: &uring_sys::__kernel_timespec,
        count: u32,
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

    #[inline]
    pub unsafe fn prep_fadvise(&mut self, fd: RawFd, off: u64, len: u64, advice: PosixFadviseAdvice) {
        use PosixFadviseAdvice::*;
        let advice = match advice {
            POSIX_FADV_NORMAL       => libc::POSIX_FADV_NORMAL,
            POSIX_FADV_SEQUENTIAL   => libc::POSIX_FADV_SEQUENTIAL,
            POSIX_FADV_RANDOM       => libc::POSIX_FADV_RANDOM,
            POSIX_FADV_NOREUSE      => libc::POSIX_FADV_NOREUSE,
            POSIX_FADV_WILLNEED     => libc::POSIX_FADV_WILLNEED,
            POSIX_FADV_DONTNEED     => libc::POSIX_FADV_DONTNEED,
        };
        uring_sys::io_uring_prep_fadvise(self.sqe, fd, off as _, len as _, advice);
    }

    #[inline]
    pub unsafe fn prep_madvise(&mut self, data: &mut [u8], advice: MmapAdvise) {
        use MmapAdvise::*;
        let advice = match advice {
            MADV_NORMAL         => libc::MADV_NORMAL,
            MADV_RANDOM         => libc::MADV_RANDOM,
            MADV_SEQUENTIAL     => libc::MADV_SEQUENTIAL,
            MADV_WILLNEED       => libc::MADV_WILLNEED,
            MADV_DONTNEED       => libc::MADV_DONTNEED,
            MADV_REMOVE         => libc::MADV_REMOVE,
            MADV_DONTFORK       => libc::MADV_DONTFORK,
            MADV_DOFORK         => libc::MADV_DOFORK,
            MADV_HWPOISON       => libc::MADV_HWPOISON,
            MADV_MERGEABLE      => libc::MADV_MERGEABLE,
            MADV_UNMERGEABLE    => libc::MADV_UNMERGEABLE,
            MADV_SOFT_OFFLINE   => libc::MADV_SOFT_OFFLINE,
            MADV_HUGEPAGE       => libc::MADV_HUGEPAGE,
            MADV_NOHUGEPAGE     => libc::MADV_NOHUGEPAGE,
            MADV_DONTDUMP       => libc::MADV_DONTDUMP,
            MADV_DODUMP         => libc::MADV_DODUMP,
            MADV_FREE           => libc::MADV_FREE,
        };
        uring_sys::io_uring_prep_madvise(self.sqe, data.as_mut_ptr() as *mut _, data.len() as _, advice);
    }

    #[inline]
    pub unsafe fn prep_epoll_ctl(&mut self, epoll_fd: RawFd, op: EpollOp, fd: RawFd, event: Option<&mut EpollEvent>) {
        let op = match op {
            EpollOp::EpollCtlAdd    => libc::EPOLL_CTL_ADD,
            EpollOp::EpollCtlDel    => libc::EPOLL_CTL_DEL,
            EpollOp::EpollCtlMod    => libc::EPOLL_CTL_MOD,
        };
        let event = event.map_or(ptr::null_mut(), |event| event as *mut EpollEvent as *mut _);
        uring_sys::io_uring_prep_epoll_ctl(self.sqe, epoll_fd, fd, op, event);
    }

    #[inline]
    pub unsafe fn prep_files_update(&mut self, files: &[RawFd], offset: u32) {
        let addr = files.as_ptr() as *mut RawFd;
        let len = files.len() as u32;
        uring_sys::io_uring_prep_files_update(self.sqe, addr, len, offset as _);
    }

    pub unsafe fn prep_provide_buffers(&mut self,
        buffers: &mut [u8],
        count: u32,
        group: BufferGroupId,
        index: u32,
    ) {
        let addr = buffers.as_mut_ptr() as *mut libc::c_void;
        let len = buffers.len() as u32 / count;
        uring_sys::io_uring_prep_provide_buffers(self.sqe, addr, len as _, count as _, group.id as _, index as _);
    }

    pub unsafe fn prep_remove_buffers(&mut self, count: u32, id: BufferGroupId) {
        uring_sys::io_uring_prep_remove_buffers(self.sqe, count as _, id.id as _);
    }

    #[inline]
    pub unsafe fn prep_cancel(&mut self, user_data: u64, flags: i32) {
        uring_sys::io_uring_prep_cancel(self.sqe, user_data as _, flags);
    }


    /// Prepare a no-op event.
    /// ```
    /// # use iou::{IoUring, SubmissionFlags};
    /// # fn main() -> std::io::Result<()> {
    /// # let mut ring = IoUring::new(1)?;
    /// #
    /// // example: use a no-op to force a drain
    ///
    /// let mut nop = ring.prepare_sqe().unwrap();
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
    /// # let mut sqe = ring.prepare_sqe().unwrap();
    /// #
    /// unsafe { sqe.set_user_data(0x1010); }
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
    /// # let mut sqe = ring.prepare_sqe().unwrap();
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

    pub unsafe fn raw_mut(&mut self) -> &mut uring_sys::io_uring_sqe {
        &mut self.sqe
    }
}

unsafe impl<'a> Send for SQE<'a> { }
unsafe impl<'a> Sync for SQE<'a> { }

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

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BufferGroupId {
    pub id: u32,
}

bitflags::bitflags! {
    /// [`SQE`](SQE) configuration flags.
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

        const IO_HARDLINK   = 1 << 3;
        const ASYNC         = 1 << 4;
        const BUFFER_SELECT = 1 << 5;
    }
}

bitflags::bitflags! {
    pub struct FsyncFlags: u32 {
        /// Sync file data without an immediate metadata sync.
        const FSYNC_DATASYNC    = 1 << 0;
    }
}

bitflags::bitflags! {
    pub struct StatxFlags: i32 {
        const AT_STATX_SYNC_AS_STAT = 0;
        const AT_SYMLINK_NOFOLLOW   = 1 << 10;
        const AT_NO_AUTOMOUNT       = 1 << 11;
        const AT_EMPTY_PATH         = 1 << 12;
        const AT_STATX_FORCE_SYNC   = 1 << 13;
        const AT_STATX_DONT_SYNC    = 1 << 14;
    }
}

bitflags::bitflags! {
    pub struct StatxMode: i32 {
        const STATX_TYPE        = 1 << 0;
        const STATX_MODE        = 1 << 1;
        const STATX_NLINK       = 1 << 2;
        const STATX_UID         = 1 << 3;
        const STATX_GID         = 1 << 4;
        const STATX_ATIME       = 1 << 5;
        const STATX_MTIME       = 1 << 6;
        const STATX_CTIME       = 1 << 7;
        const STATX_INO         = 1 << 8;
        const STATX_SIZE        = 1 << 9;
        const STATX_BLOCKS      = 1 << 10;
        const STATX_BTIME       = 1 << 11;
    }
}

bitflags::bitflags! {
    pub struct TimeoutFlags: u32 {
        const TIMEOUT_ABS   = 1 << 0;
    }
}

bitflags::bitflags! {
    pub struct SpliceFlags: u32 {
        const F_FD_IN_FIXED = 1 << 31;
    }
}

pub struct SQEs<'ring> {
    sqes: slice::IterMut<'ring, uring_sys::io_uring_sqe>,
}

impl<'ring> SQEs<'ring> {
    pub(crate) fn new(slice: &'ring mut [uring_sys::io_uring_sqe]) -> SQEs<'ring> {
        SQEs {
            sqes: slice.iter_mut(),
        }
    }

    pub fn single(&mut self) -> Option<SQE<'ring>> {
        let mut next = None;
        while let Some(sqe) = self.consume() { next = Some(sqe) }
        next
    }

    pub fn hard_linked(&mut self) -> HardLinked<'ring, '_> {
        HardLinked { sqes: self }
    }

    pub fn remaining(&self) -> u32 {
        self.sqes.len() as u32
    }

    fn consume(&mut self) -> Option<SQE<'ring>> {
        self.sqes.next().map(|sqe| {
            unsafe { uring_sys::io_uring_prep_nop(sqe) }
            SQE { sqe }
        })
    }
}

pub struct HardLinked<'ring, 'a> {
    sqes: &'a mut SQEs<'ring>,
}

impl<'ring> HardLinked<'ring, '_> {
    pub fn terminate(self) -> Option<SQE<'ring>> {
        self.sqes.consume()
    }
}

impl<'ring> Iterator for HardLinked<'ring, '_> {
    type Item = HardLinkedSQE<'ring>;

    fn next(&mut self) -> Option<Self::Item> {
        let is_final = self.sqes.remaining() == 1;
        self.sqes.consume().map(|sqe| HardLinkedSQE { sqe, is_final })
    }
}

pub struct HardLinkedSQE<'ring> {
    sqe: SQE<'ring>,
    is_final: bool,
}

impl<'ring> Deref for HardLinkedSQE<'ring> {
    type Target = SQE<'ring>;

    fn deref(&self) -> &SQE<'ring> {
        &self.sqe
    }
}

impl<'ring> DerefMut for HardLinkedSQE<'ring> {
    fn deref_mut(&mut self) -> &mut SQE<'ring> {
        &mut self.sqe
    }
}

impl<'ring> Drop for HardLinkedSQE<'ring> {
    fn drop(&mut self) {
        if !self.is_final {
            self.sqe.set_flags(SubmissionFlags::IO_HARDLINK);
        }
    }
}
