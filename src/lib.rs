//! Idiomatic Rust bindings to liburing.
//!
//! This gives users an idiomatic Rust interface for interacting with the Linux kernel's `io_uring`
//! interface for async IO. Despite being idiomatic Rust, this interface is still very low level
//! and some fundamental operations remain unsafe.
//!
//! The core entry point to the API is the `IoUring` type, which manages an `io_uring` object for
//! interfacing with the kernel. Using this, users can submit IO events and wait for their
//! completion.
//!
//! It is also possible to "split" an `IoUring` instance into its constituent components - a
//! `SubmissionQueue`, a `CompletionQueue`, and a `Registrar` - in order to operate on them
//! separately without synchronization.
//!
//! # Submitting events
//!
//! You can prepare new IO events using the `SubmissionQueueEvent` type. Once an event has been
//! prepared, the next call to submit will submit that event. Eventually, those events will 
//! complete, and that a `CompletionQueueEvent` will appear on the completion queue indicating that
//! the event is complete.
//!
//! Preparing IO events is inherently unsafe, as you must guarantee that the buffers and file
//! descriptors used for that IO are alive long enough for the kernel to perform the IO operation
//! with them.
//!
//! # Timeouts
//!
//! Some APIs allow you to time out a call into the kernel. It's important to note how this works
//! with io_uring.
//!
//! A timeout is submitted as an additional IO event which completes after the specified time.
//! Therefore when you create a timeout, all that happens is that a completion event will appear
//! after that specified time. This also means that when processing completion events, you need to
//! be prepared for the possibility that the completion represents a timeout and not a normal IO
//! event (`CompletionQueueEvent` has a method to check for this).

macro_rules! resultify {
    ($ret:expr) => {
        {
            let ret = $ret;
            match ret >= 0 {
                true    => Ok(ret as _),
                false   => Err(std::io::Error::from_raw_os_error(-ret)),
            }
        }
    }
}

mod cqe;
mod sqe;
mod registrar;

#[doc(inline)]
pub use uring_sys as sys;

use std::io;
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};
use std::time::Duration;

pub use sqe::{SubmissionQueue, SubmissionQueueEvent, SubmissionFlags, FsyncFlags};
pub use cqe::{CompletionQueue, CompletionQueueEvent};
pub use registrar::Registrar;

bitflags::bitflags! {
    pub struct SetupFlags: u32 {
        const IOPOLL    = 1 << 0;   /* io_context is polled */
        const SQPOLL    = 1 << 1;   /* SQ poll thread */
        const SQ_AFF    = 1 << 2;   /* sq_thread_cpu is valid */
    }
}

/// The main interface to kernel IO using `io_uring`.
///
/// `IoUring` is a high-level wrapper around an [`io_uring`](sys::io_uring) object.
///
/// `IoUring`s are constructed with a requested number of ring buffer entries and possibly a set of
/// [`SetupFlags`](SetupFlags).
/// ```
/// # use std::io;
/// # use iou::{IoUring, SetupFlags};
/// # fn main() -> io::Result<()> {
/// // make a IoUring with 16 entries
/// let mut ring = IoUring::new(16)?;
///
/// // make a IoUring set to poll the IO context
/// let mut ring = IoUring::new_with_flags(32, SetupFlags::IOPOLL)?;
/// # Ok(())
/// # }
/// ```
///
/// `IoUring`s can either be used directly, or split into separate parts and
/// operated on without synchronization.
/// ```
/// # use std::io;
/// # use iou::{IoUring, SetupFlags, CompletionQueue, SubmissionQueue, Registrar};
/// # fn main() -> io::Result<()> {
/// # let mut ring = IoUring::new(32)?;
/// // split an IoUring piecewise
/// let sq: SubmissionQueue = ring.sq();
/// let cq: CompletionQueue = ring.cq();
/// let reg: Registrar = ring.registrar();
///
/// // split an IoUring into its three parts all at once
/// let (sq, cq, reg) = ring.queues();
/// # Ok(())
/// # }
/// ```
pub struct IoUring {
    ring: sys::io_uring,
}

impl IoUring {
    /// Creates a new `IoUring` without any setup flags.
    ///
    /// The number of entries must be in the range of 1..4096 (inclusive) and
    /// it's recommended to be a power of two.
    ///
    /// The underlying `SubmissionQueue` and `CompletionQueue` will each have this number of
    /// entries.
    pub fn new(entries: u32) -> io::Result<IoUring> {
        IoUring::new_with_flags(entries, SetupFlags::empty())
    }

    /// Creates a new `IoUring` using a set of `SetupFlags` for advanced usage.
    pub fn new_with_flags(entries: u32, flags: SetupFlags) -> io::Result<IoUring> {
        unsafe {
            let mut ring = MaybeUninit::uninit();
            let _: i32 = resultify! {
                sys::io_uring_queue_init(entries as _, ring.as_mut_ptr(), flags.bits() as _)
            }?;
            Ok(IoUring { ring: ring.assume_init() })
        }
    }

    /// Returns the `SubmissionQueue` part of the `IoUring`.
    pub fn sq(&mut self) -> SubmissionQueue<'_> {
        SubmissionQueue::new(&*self)
    }

    /// Returns the `CompletionQueue` part of the `IoUring`.
    pub fn cq(&mut self) -> CompletionQueue<'_> {
        CompletionQueue::new(&*self)
    }

    /// Returns the `Registrar` part of the `IoUring`.
    pub fn registrar(&self) -> Registrar<'_> {
        Registrar::new(self)
    }

    /// Returns the three constituent parts of the `IoUring`.
    pub fn queues(&mut self) -> (SubmissionQueue<'_>, CompletionQueue<'_>, Registrar<'_>) {
        (SubmissionQueue::new(&*self), CompletionQueue::new(&*self), Registrar::new(&*self))
    }

    pub fn next_sqe(&mut self) -> Option<SubmissionQueueEvent<'_>> {
        unsafe {
            let sqe = sys::io_uring_get_sqe(&mut self.ring);
            if sqe != ptr::null_mut() {
                let mut sqe = SubmissionQueueEvent::new(&mut *sqe);
                sqe.clear();
                Some(sqe)
            } else {
                None
            }
        }
    }

    pub fn submit_sqes(&mut self) -> io::Result<usize> {
        self.sq().submit()
    }

    pub fn submit_sqes_and_wait(&mut self, wait_for: u32) -> io::Result<usize> {
        self.sq().submit_and_wait(wait_for)
    }

    pub fn submit_sqes_and_wait_with_timeout(&mut self, wait_for: u32, duration: Duration)
        -> io::Result<usize>
    {
        self.sq().submit_and_wait_with_timeout(wait_for, duration)
    }

    pub fn peek_for_cqe(&mut self) -> Option<CompletionQueueEvent<'_>> {
        unsafe {
            let mut cqe = MaybeUninit::uninit();
            sys::io_uring_peek_batch_cqe(&mut self.ring, cqe.as_mut_ptr(), 1);
            let cqe = cqe.assume_init();
            if cqe != ptr::null_mut() {
                Some(CompletionQueueEvent::new(NonNull::from(&self.ring), &mut *cqe))
            } else {
                None
            }
        }
    }

    pub fn wait_for_cqe(&mut self) -> io::Result<CompletionQueueEvent<'_>> {
        self.inner_wait_for_cqes(1, ptr::null())
    }

    pub fn wait_for_cqe_with_timeout(&mut self, duration: Duration)
        -> io::Result<CompletionQueueEvent<'_>>
    {
        let ts = sys::__kernel_timespec {
            tv_sec: duration.as_secs() as _,
            tv_nsec: duration.subsec_nanos() as _
        };

        self.inner_wait_for_cqes(1, &ts)
    }

    pub fn wait_for_cqes(&mut self, count: usize) -> io::Result<CompletionQueueEvent<'_>> {
        self.inner_wait_for_cqes(count as _, ptr::null())
    }

    pub fn wait_for_cqes_with_timeout(&mut self, count: usize, duration: Duration)
        -> io::Result<CompletionQueueEvent<'_>>
    {
        let ts = sys::__kernel_timespec {
            tv_sec: duration.as_secs() as _,
            tv_nsec: duration.subsec_nanos() as _
        };

        self.inner_wait_for_cqes(count as _, &ts)
    }

    fn inner_wait_for_cqes(&mut self, count: u32, ts: *const sys::__kernel_timespec)
        -> io::Result<CompletionQueueEvent<'_>>
    {
        unsafe {
            let mut cqe = MaybeUninit::uninit();

            let _: i32 = resultify!(sys::io_uring_wait_cqes(
                &mut self.ring,
                cqe.as_mut_ptr(),
                count,
                ts,
                ptr::null(),
            ))?;

            Ok(CompletionQueueEvent::new(NonNull::from(&self.ring), &mut *cqe.assume_init()))
        }
    }

    pub fn raw(&self) -> &sys::io_uring {
        &self.ring
    }

    pub fn raw_mut(&mut self) -> &mut sys::io_uring {
        &mut self.ring
    }
}

impl Drop for IoUring {
    fn drop(&mut self) {
        unsafe { sys::io_uring_queue_exit(&mut self.ring) };
    }
}

unsafe impl Send for IoUring { }
unsafe impl Sync for IoUring { }

// This has to live in an inline module to test the non-exported resultify macro.
#[cfg(test)]
mod tests {
    #[test]
    fn test_resultify() {
        let side_effect = |i, effect: &mut _| -> i32 {
            *effect += 1;
            return i;
        };

        let mut calls = 0;
        let ret: Result<i32, _> = resultify!(side_effect(0, &mut calls));
        assert!(match ret { Ok(0) => true, _ => false });
        assert_eq!(calls, 1);

        calls = 0;
        let ret: Result<i32, _> = resultify!(side_effect(1, &mut calls));
        assert!(match ret { Ok(1) => true, _ => false });
        assert_eq!(calls, 1);

        calls = 0;
        let ret: Result<i32, _> = resultify!(side_effect(-1, &mut calls));
        assert!(match ret { Err(e) if e.raw_os_error() == Some(1) => true, _ => false });
        assert_eq!(calls, 1);
    }
}
