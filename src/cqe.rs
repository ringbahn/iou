use std::io;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};

use super::{IoUring, SubmissionQueue};

/// The queue of completed IO events.
///
/// Each element is a [`CompletionQueueEvent`](crate::cqe::CompletionQueueEvent).
///
/// Completion does not imply success. Completed events may be [timeouts](crate::cqe::CompletionQueueEvent::is_timeout).
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

    pub fn peek_for_cqe(&mut self) -> Option<CompletionQueueEvent<'_>> {
        unsafe { CompletionQueueEvent::peek(self.ring) }
    }

    pub fn wait_for_cqe(&mut self) -> io::Result<CompletionQueueEvent<'_>> {
        let mut cqe = None;
        while cqe.is_none() {
            unsafe {
                cqe = CompletionQueueEvent::wait(self.ring, ptr::null())?;
            }
        }
        Ok(cqe.unwrap())
    }

    pub fn wait_for_cqe_with_timeout<'a>(
        &'a mut self,
        sq: &mut SubmissionQueue<'ring>,
        duration: std::time::Duration,
    ) -> io::Result<Option<CompletionQueueEvent<'a>>> {
        assert_eq!(self.ring.as_ptr() as usize, sq.ring().as_ptr() as usize);

        let ts = crate::timespec(duration);
        unsafe { CompletionQueueEvent::wait(self.ring, &ts) }
    }

    pub fn peek_for_cqes(&mut self) -> CompletionQueueEvents<'_> {
        unsafe { CompletionQueueEvents::peek(self.ring) }
    }

    pub fn wait_for_cqes(&mut self, count: usize) -> io::Result<CompletionQueueEvents<'_>> {
        unsafe { CompletionQueueEvents::wait(self.ring, count, ptr::null()) }
    }

    pub fn wait_for_cqes_with_timeout<'a>(
        &'a mut self,
        sq: &mut SubmissionQueue<'ring>,
        count: usize,
        duration: std::time::Duration,
    ) -> io::Result<CompletionQueueEvents<'a>> {
        assert_eq!(self.ring.as_ptr() as usize, sq.ring().as_ptr() as usize);

        let ts = crate::timespec(duration);
        unsafe { CompletionQueueEvents::wait(self.ring, count, &ts) }
    }
}

unsafe impl<'ring> Send for CompletionQueue<'ring> { }
unsafe impl<'ring> Sync for CompletionQueue<'ring> { }

pub struct CompletionQueueEvents<'a, F = fn(io::Result<usize>)> {
    ring: NonNull<uring_sys::io_uring>,
    ptr: *mut uring_sys::io_uring_cqe,
    available: usize,
    seen: usize,
    timeout_handler: Option<F>,
    _marker: PhantomData<&'a mut IoUring>,
}

impl<'a, F: FnMut(io::Result<usize>)> CompletionQueueEvents<'a, F> {
    // unsafe contract:
    //  - ring must not be dangling
    //  - this returns a CQE iterator with an arbitrary lifetime, you must have logically exclusive
    //    access to the CQ for that lifetime
    //  - if ts is nonnull, you must have logically exclusive access to the SQ as well as CQ and ts
    //    must point to a valid __kernel_timespec
    pub(crate) unsafe fn wait(
        ring: NonNull<uring_sys::io_uring>,
        count: usize,
        ts: *const uring_sys::__kernel_timespec
    ) -> io::Result<CompletionQueueEvents<'a, F>> {
        let mut cqe = MaybeUninit::uninit();
        let available = wait(ring, &mut cqe, count, ts)?;
        if available != 0 {
            Ok(CompletionQueueEvents {
                ring: ring,
                available,
                ptr: cqe.assume_init(),
                seen: 0,
                timeout_handler: None,
                _marker: PhantomData,
            })
        } else {
            Ok(CompletionQueueEvents::peek(ring))
        }
    }

    // unsafe contract:
    //  - ring must not be dangling
    //  - this returns a CQE iterator with an arbitrary lifetime, you must have logically exclusive
    //    access to the CQ for that lifetime
    pub(crate) unsafe fn peek(ring: NonNull<uring_sys::io_uring>) -> CompletionQueueEvents<'a, F> {
        CompletionQueueEvents {
            ring,
            ptr: ptr::null_mut(),
            available: 0,
            seen: 0,
            timeout_handler: None,
            _marker: PhantomData,
        }
    }

    pub fn next_cqe(&mut self) -> Option<CompletionQueueEvent<'_>> {
        'skip_timeouts: loop {
            unsafe {
                // If none are available, peek to see if there are more, resetting
                // the number of available CQEs to the total number of ready CQEs
                // minus the number of seen CQEs.
                if self.available == 0 {
                    let mut cqe = MaybeUninit::uninit();
                    let ready = peek(self.ring, &mut cqe);

                    self.available = ready - self.seen;

                    // If there are still none available, return None
                    if self.available == 0 {
                        return None;
                    }

                    // Otherwise, if self.ptr is null (meaning we have never
                    // returned a CQE yet), set it to be the first available
                    // CQE.
                    if self.ptr == ptr::null_mut() {
                        self.ptr = cqe.assume_init();
                    }
                }

                // Construct a CQE from self.ptr, now that we know there is at least one more
                // CQE available and our pointer is non-null. We pass a null pointer for the
                // ring so that it will not advance the queue on drop.
                let cqe = CompletionQueueEvent {
                    ring: ptr::null_mut(),
                    cqe: &mut *self.ptr
                };

                // Advance the pointer and our counters because we have now taken this pointer
                self.ptr = self.ptr.offset(1);
                self.available -= 1;
                self.seen += 1;

                // If this CQE is a timeout, repeat this process. Otherwise, return this CQE.
                if cqe.is_timeout() {
                    if let Some(handler) = &mut self.timeout_handler {
                        handler(cqe.result())
                    }

                    continue 'skip_timeouts;
                }
                
                return Some(cqe);
            }
        }
    }

    pub fn for_each(&mut self, mut f: impl FnMut(CompletionQueueEvent<'_>)) {
        while let Some(cqe) = self.next_cqe() {
            f(cqe);
        }
    }

    pub fn try_for_each<E>(&mut self, mut f: impl FnMut(CompletionQueueEvent<'_>) -> Result<(), E>)
        -> Result<(), E>
    {
        while let Some(cqe) = self.next_cqe() {
            f(cqe)?;
        }
        Ok(())
    }

    pub fn wait_for_cqes(&mut self, count: usize) -> io::Result<()> {
        unsafe {
            let mut cqe = MaybeUninit::uninit();
            let ready = wait(self.ring, &mut cqe, count + self.seen, ptr::null())?;

            self.available = ready - self.seen;

            if self.available != 0 && self.ptr == ptr::null_mut() {
                self.ptr = cqe.assume_init();
            }

            Ok(())
        }
    }

    pub fn advance_queue(&mut self) {
        unsafe {
            uring_sys::io_uring_cq_advance(self.ring.as_ptr(), self.seen as _);
        }
        self.seen = 0;
    }

    pub fn handle_timeouts(&mut self, handler: F) -> &mut Self {
        self.timeout_handler = Some(handler);
        self
    }
}

impl<'a, F> Drop for CompletionQueueEvents<'a, F> {
    fn drop(&mut self) {
        // Advance the CQ by as many CQEs as we have seen using this iterator.
        unsafe {
            uring_sys::io_uring_cq_advance(self.ring.as_ptr(), self.seen as _);
        }
    }
}

unsafe impl<'a> Send for CompletionQueueEvents<'a> { }
unsafe impl<'a> Sync for CompletionQueueEvents<'a> { }

/// A completed IO event.
pub struct CompletionQueueEvent<'a> {
    ring: *mut uring_sys::io_uring,
    cqe: &'a mut uring_sys::io_uring_cqe,
}

impl<'a> CompletionQueueEvent<'a> {
    // unsafe contract:
    //  - ring must not be dangling
    //  - this returns a CQE with an arbitrary lifetime, you must have logically exclusive access
    //    to the CQ for that lifetime
    pub(crate) unsafe fn peek(ring: NonNull<uring_sys::io_uring>)
        -> Option<CompletionQueueEvent<'a>>
    {
        let mut cqe = MaybeUninit::uninit();
        uring_sys::io_uring_peek_cqe(ring.as_ptr(), cqe.as_mut_ptr());
        let cqe = cqe.assume_init();
        if cqe != ptr::null_mut() {
            Some(CompletionQueueEvent {
                ring: ring.as_ptr(),
                cqe: &mut *cqe,
            })
        } else {
            None
        }
    }

    // unsafe contract:
    //  - ring must not be dangling
    //  - this returns a CQE with an arbitrary lifetime, you must have logically exclusive access
    //    to the CQ for that lifetime
    //  - if ts is nonnull, you must have logically exclusive access to the SQ as well as CQ and ts
    //    must point to a valid __kernel_timespec
    pub(crate) unsafe fn wait(
        ring: NonNull<uring_sys::io_uring>,
        ts: *const uring_sys::__kernel_timespec
    ) -> io::Result<Option<CompletionQueueEvent<'a>>> {
        let mut cqe = MaybeUninit::uninit();

        let _: i32 = resultify!(uring_sys::io_uring_wait_cqe_timeout(
            ring.as_ptr(),
            cqe.as_mut_ptr(),
            ts as _,
        ))?;

        let cqe = CompletionQueueEvent {
            ring: ring.as_ptr(),
            cqe: &mut *cqe.assume_init(),
        };

        if  !cqe.is_timeout() {
            Ok(Some(cqe))
        } else {
            Ok(None)
        }
    }

    pub fn user_data(&self) -> u64 {
        self.cqe.user_data as u64
    }

    pub fn result(&self) -> io::Result<usize> {
        resultify!(self.cqe.res)
    }

    pub fn raw(&self) -> &uring_sys::io_uring_cqe {
        self.cqe
    }

    fn is_timeout(&self) -> bool {
        self.cqe.user_data == uring_sys::LIBURING_UDATA_TIMEOUT
    }
}

impl<'a> Drop for CompletionQueueEvent<'a> {
    fn drop(&mut self) {
        if self.ring != ptr::null_mut() {
            unsafe {
                uring_sys::io_uring_cqe_seen(self.ring, self.cqe)
            }
        }
    }
}

unsafe impl<'a> Send for CompletionQueueEvent<'a> { }
unsafe impl<'a> Sync for CompletionQueueEvent<'a> { }

// unsafe contract:
//  - ring must not be dangling
//  - you must have logically exclusive access to the CQ for this function call
//
// NOTE: The pointer offsetting is hand-written because there is no API currently in liburing that
// returns the next CQE and also an accurate count of how many CQEs are ready in only one
// synchronization.
pub(crate) unsafe fn peek<'a>(
    ring: NonNull<uring_sys::io_uring>,
    cqe: &mut MaybeUninit<*mut uring_sys::io_uring_cqe>
) -> usize
{
    let ring = ring.as_ptr();
    let count = uring_sys::io_uring_cq_ready(ring);

    if count != 0 {
        let head = *(*ring).cq.khead as usize;
        let mask = *(*ring).cq.kring_mask as usize;
        *cqe.as_mut_ptr() = (*ring).cq.cqes.offset((head & mask) as isize);
    }

    count as usize
}

// unsafe contract:
//  - ring must not be dangling
//  - you must have logically exclusive access to the CQ for this function call
//  - if ts is nonnull, you must have logically exclusive access to the SQ as well as CQ and ts
//    must point to a valid __kernel_timespec
#[inline(always)]
pub(crate) unsafe fn wait(
    ring: NonNull<uring_sys::io_uring>,
    cqe: &mut MaybeUninit<*mut uring_sys::io_uring_cqe>,
    count: usize,
    ts: *const uring_sys::__kernel_timespec
) -> io::Result<usize> {
    let ring = ring.as_ptr();
    let cqe = cqe.as_mut_ptr();
    resultify!(uring_sys::io_uring_wait_cqes(ring, cqe, count as _, ts, ptr::null()))
}
