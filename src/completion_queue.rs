use std::io;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};

use super::{IoUring, CQE, CQEs, CQEsBlocking, resultify};

/// The queue of completed IO events.
///
/// Each element is a [`CQE`](crate::cqe::CQE).
///
/// Completion does not imply success. Completed events may be [timeouts](crate::cqe::CQE::is_timeout).
pub struct CompletionQueue<'ring> {
    ring: NonNull<uring_sys::io_uring>,
    _marker: PhantomData<&'ring mut IoUring>,
}

impl<'ring> CompletionQueue<'ring> {
    pub(crate) fn new(ring: &'ring IoUring) -> CompletionQueue<'ring> {
        CompletionQueue {
            ring: NonNull::from(&ring.ring),
            _marker: PhantomData,
        }
    }

    pub fn peek_for_cqe(&mut self) -> Option<CQE> {
        unsafe {
            let mut cqe = MaybeUninit::uninit();
            let count = uring_sys::io_uring_peek_batch_cqe(self.ring.as_ptr(), cqe.as_mut_ptr(), 1);
            if count > 0 {
                Some(CQE::new(self.ring, &mut *cqe.assume_init()))
            } else {
                None
            }
        }
    }

    pub fn wait_for_cqe(&mut self) -> io::Result<CQE> {
        self.wait_for_cqes(1)
    }

    pub fn wait_for_cqes(&mut self, count: u32) -> io::Result<CQE> {
        unsafe {
            let mut cqe = MaybeUninit::uninit();

            resultify(uring_sys::io_uring_wait_cqes(
                self.ring.as_ptr(),
                cqe.as_mut_ptr(),
                count as _,
                ptr::null(),
                ptr::null(),
            ))?;

            Ok(CQE::new(self.ring, &mut *cqe.assume_init()))
        }
    }

    pub fn cqes(&mut self) -> CQEs<'ring, '_> {
        CQEs {
            queue: self,
            ready: 0,
        }
    }

    pub fn cqes_blocking(&mut self) -> CQEsBlocking<'ring, '_> {
        CQEsBlocking {
            queue: self,
            ready: 0,
        }
    }

    pub fn ready(&self) -> u32 {
        unsafe { uring_sys::io_uring_cq_ready(self.ring.as_ptr()) }
    }

    pub fn eventfd_enabled(&self) -> bool {
        unsafe { uring_sys::io_uring_cq_eventfd_enabled(self.ring.as_ptr()) }
    }

    pub fn eventfd_toggle(&mut self, enabled: bool) -> io::Result<()> {
        resultify(unsafe { uring_sys::io_uring_cq_eventfd_toggle(self.ring.as_ptr(), enabled) })?;
        Ok(())
    }
}

unsafe impl<'ring> Send for CompletionQueue<'ring> { }
unsafe impl<'ring> Sync for CompletionQueue<'ring> { }

