use std::io;
use std::mem;
use std::os::unix::io::RawFd;
use std::ptr::{self, NonNull};
use std::marker::PhantomData;
use std::time::Duration;

use super::{IoUring, sys};

const IORING_OP_NOP:            u8  = 0;
const IORING_OP_READV:          u8  = 1;
const IORING_OP_WRITEV:         u8  = 2;
const IORING_OP_FSYNC:          u8  = 3;
const IORING_OP_READ_FIXED:     u8  = 4;
const IORING_OP_WRITE_FIXED:    u8  = 5;
const IORING_OP_TIMEOUT:        u8  = 11;

pub struct SubmissionQueue<'ring> {
    ring: NonNull<sys::io_uring>,
    _marker: PhantomData<&'ring mut IoUring>,
}

impl<'ring> SubmissionQueue<'ring> {
    pub(crate) fn new(ring: &'ring IoUring) -> SubmissionQueue<'ring> {
        SubmissionQueue {
            ring: NonNull::from(&ring.ring),
            _marker: PhantomData,
        }
    }

    pub fn next_sqe<'a>(&'a mut self) -> Option<SubmissionQueueEvent<'a>> {
        unsafe {
            let sqe = sys::io_uring_get_sqe(self.ring.as_ptr());
            if sqe != ptr::null_mut() {
                let sqe = &mut *sqe;
                sqe.clear();
                Some(SubmissionQueueEvent::new(sqe))
            } else {
                None
            }
        }
    }

    pub fn submit(&mut self) -> io::Result<usize> {
        let ret = unsafe { sys::io_uring_submit(self.ring.as_ptr()) };
        if ret >= 0 {
            Ok(ret as _)
        } else {
            Err(io::Error::from_raw_os_error(ret))
        }
    }

    pub fn submit_and_wait(&mut self, wait_for: u32) -> io::Result<usize> {
        let ret = unsafe { sys::io_uring_submit_and_wait(self.ring.as_ptr(), wait_for as _) };
        if ret >= 0 {
            Ok(ret as _)
        } else {
            Err(io::Error::from_raw_os_error(ret))
        }
    }

    pub fn submit_and_wait_with_timeout(&mut self, wait_for: u32, duration: Duration)
        -> io::Result<usize>
    {
        let ts = sys::__kernel_timespec {
            tv_sec: duration.as_secs() as _,
            tv_nsec: duration.subsec_nanos() as _
        };

        loop {
            if let Some(mut sqe) = self.next_sqe() {
                sqe.clear();
                unsafe { sqe.prep_timeout(&ts); }
                let ret = unsafe { sys::io_uring_submit_and_wait(self.ring.as_ptr(), wait_for as _) };

                if ret >= 0 {
                    return Ok(ret as _)
                } else {
                    return Err(io::Error::from_raw_os_error(ret))
                }
            }

            self.submit()?;
        }
    }
}

unsafe impl<'ring> Send for SubmissionQueue<'ring> { }
unsafe impl<'ring> Sync for SubmissionQueue<'ring> { }

pub struct SubmissionQueueEvent<'a> {
    sqe: &'a mut sys::io_uring_sqe,
}

impl<'a> SubmissionQueueEvent<'a> {
    pub(crate) fn new(sqe: &'a mut sys::io_uring_sqe) -> SubmissionQueueEvent<'a> {
        SubmissionQueueEvent { sqe }
    }

    pub fn user_data(&self) -> u64 {
        self.sqe.user_data as u64
    }

    pub fn set_user_data(&mut self, user_data: u64) {
        self.sqe.user_data = user_data as _;
    }

    pub fn flags(&self) -> SubmissionFlags {
        unsafe { SubmissionFlags::from_bits_unchecked(self.sqe.flags as _) }
    }

    pub fn set_flags(&mut self, flags: SubmissionFlags) {
        self.sqe.flags = flags.bits() as _;
    }

    #[inline]
    pub unsafe fn prep_read_vectored(
        &mut self,
        fd: RawFd,
        bufs: &mut [io::IoSliceMut<'_>],
        offset: usize,
    ) {
        let len = bufs.len();
        let addr = bufs as *mut [io::IoSliceMut<'_>] as *mut io::IoSliceMut<'_>;
        self.sqe.opcode = IORING_OP_READV as _;
        self.sqe.fd = fd;
        self.sqe.off_addr2.off = offset as _;
        self.sqe.addr = addr as _;
        self.sqe.len = len as _;
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
        let addr = buf as *mut [u8] as *mut u8;
        self.sqe.opcode = IORING_OP_READ_FIXED as _;
        self.sqe.fd = fd;
        self.sqe.off_addr2.off = offset as _;
        self.sqe.addr = addr as _;
        self.sqe.len = len as _;
        self.sqe.buf_index.buf_index = buf_index as _;
        self.sqe.flags |= SubmissionFlags::FIXED_FILE.bits();
    }

    #[inline]
    pub unsafe fn prep_write_vectored(
        &mut self,
        fd: RawFd,
        bufs: &[io::IoSlice<'_>],
        offset: usize,
    ) {
        let len = bufs.len();
        let addr = bufs as *const [io::IoSlice<'_>] as *const io::IoSlice<'_>;
        self.sqe.opcode = IORING_OP_WRITEV as _;
        self.sqe.fd = fd;
        self.sqe.off_addr2.off = offset as _;
        self.sqe.addr = addr as _;
        self.sqe.len = len as _;
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
        let addr = buf as *const [u8] as *const u8;
        self.sqe.opcode = IORING_OP_WRITE_FIXED as _;
        self.sqe.fd = fd;
        self.sqe.off_addr2.off = offset as _;
        self.sqe.addr = addr as _;
        self.sqe.len = len as _;
        self.sqe.buf_index.buf_index = buf_index as _;
        self.sqe.flags |= SubmissionFlags::FIXED_FILE.bits();
    }

    #[inline]
    pub unsafe fn prep_fsync(&mut self, fd: RawFd, flags: FsyncFlags) {
        self.sqe.opcode = IORING_OP_FSYNC as _;
        self.sqe.fd = fd;
        self.sqe.off_addr2.off = 0;
        self.sqe.addr = 0;
        self.sqe.len = 0;
        self.sqe.cmd_flags.fsync_flags = flags.bits() as _;
    }

    #[inline]
    pub unsafe fn prep_timeout(&mut self, ts: &sys::__kernel_timespec) {
        self.sqe.opcode = IORING_OP_TIMEOUT as _;
        self.sqe.fd = 0;
        self.sqe.addr = ts as *const sys::__kernel_timespec as _;
        self.sqe.len = 1;
        self.sqe.user_data = sys::LIBURING_UDATA_TIMEOUT;
    }

    #[inline]
    pub unsafe fn prep_nop(&mut self) {
        self.sqe.opcode = IORING_OP_NOP;
        self.sqe.fd = 0;
        self.sqe.off_addr2.off = 0;
        self.sqe.addr = 0;
        self.sqe.len = 0;
    }

    pub fn clear(&mut self) {
        *self.sqe = unsafe { mem::zeroed() };
    }

    pub fn raw(&self) -> &sys::io_uring_sqe {
        &self.sqe
    }

    pub fn raw_mut(&mut self) -> &mut sys::io_uring_sqe {
        &mut self.sqe
    }
}

unsafe impl<'a> Send for SubmissionQueueEvent<'a> { }
unsafe impl<'a> Sync for SubmissionQueueEvent<'a> { }

bitflags::bitflags! {
    pub struct SubmissionFlags: u8 {
        const FIXED_FILE    = 1 << 0;   /* use fixed fileset */
        const IO_DRAIN      = 1 << 1;   /* issue after inflight IO */
        const IO_LINK       = 1 << 2;   /* next IO depends on this one */
    }
}

bitflags::bitflags! {
    pub struct FsyncFlags: u32 {
        const FSYNC_DATASYNC    = 1 << 0;
    }
}
