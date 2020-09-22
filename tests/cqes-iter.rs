// test CQE iterators

#[test]
fn cqes_nonblocking() {
    let mut io_uring = iou::IoUring::new(8).unwrap();

    for mut sqe in io_uring.prepare_sqes(8).unwrap() {
        unsafe {
            sqe.prep_nop();
            sqe.set_user_data(0);
        }
    }

    io_uring.submit_sqes_and_wait(8).unwrap();

    assert_eq!(io_uring.cqes().count(), 8);
}

#[test]
fn cqes_blocking() {
    let mut io_uring = iou::IoUring::new(8).unwrap();

    for mut sqe in io_uring.prepare_sqes(8).unwrap() {
        unsafe {
            sqe.prep_nop();
            sqe.set_user_data(0);
        }
    }

    io_uring.submit_sqes().unwrap();

    assert_eq!(io_uring.cqes_blocking(1).take(8).count(), 8);
}
