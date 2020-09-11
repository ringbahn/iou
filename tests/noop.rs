use std::io;

#[test]
fn noop_test() -> io::Result<()> {
    // confirm that setup and mmap work
    let mut io_uring = iou::IoUring::new(32)?;

    // confirm that submit and enter work
    unsafe {
        let mut sqe = io_uring.next_sqe().unwrap();
        sqe.prep_nop();
        sqe.set_user_data(0xDEADBEEF);
    }
    io_uring.submit_sqes()?;

    // confirm that cq reading works
    {
        let cqe = io_uring.wait_for_cqe()?;
        assert_eq!(cqe.user_data(), 0xDEADBEEF);
    }

    Ok(())
}
