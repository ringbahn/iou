use std::io;

#[test]
fn noop_test() -> io::Result<()> {
    // confirm that setup and mmap work
    let mut io_uring = iou::IoUring::new(32)?;

    // confirm that submit and enter work
    unsafe {
        let mut sq = io_uring.sq();
        let mut sqe = sq.next_sqe().unwrap();
        sqe.prep_nop();
        sqe.set_user_data(0xDEADBEEF);
    }
    io_uring.sq().submit()?;

    // confirm that cq reading works
    {
        let mut cq = io_uring.cq();
        let cqe = cq.wait_for_cqe()?;
        assert_eq!(cqe.user_data(), 0xDEADBEEF);
    }

    Ok(())
}
