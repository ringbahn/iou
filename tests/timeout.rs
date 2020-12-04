use iou::sqe::TimeoutFlags;

#[test]
#[should_panic(expected = "Timer expired")]
fn timeout_test() {
    let mut io_uring = iou::IoUring::new(2).unwrap();
    let mut sq = io_uring.sq();
    let mut sqe = sq.prepare_sqe().unwrap();

    // make a timeout
    let timeout_spec: _ = uring_sys::__kernel_timespec {
        tv_sec:  0 as _,
        tv_nsec: 2e4 as _,
    };

    unsafe { sqe.prep_timeout(&timeout_spec, 0, TimeoutFlags::empty()); }
    io_uring.sq().submit().unwrap();

    let mut cq = io_uring.cq();
    let cqe = cq.wait_for_cqe().unwrap();
    let _ = cqe.result().unwrap(); // panics with ETIME
}
