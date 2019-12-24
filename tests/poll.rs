use std::{io::{self, Write, Read}, os::unix::{io::AsRawFd, net}};
const MESSAGE: &'static [u8] = b"Hello World";

#[test]
fn test_poll_add() -> io::Result<()> {
    let mut ring = iou::IoUring::new(2)?;
    let (mut read, mut write) = net::UnixStream::pair()?;
    unsafe {
        let mut sqe = ring.next_sqe().expect("failed to get sqe");
        sqe.prep_poll_add(read.as_raw_fd(), iou::PollFlags::POLLIN);
        sqe.set_user_data(0xDEADBEEF);
        ring.submit_sqes()?;
    }

    write.write(MESSAGE)?;

    let cqe = ring.wait_for_cqe()?;
    assert_eq!(cqe.user_data(), 0xDEADBEEF);
    let mask = unsafe { iou::PollFlags::from_bits_unchecked(cqe.result()? as _) };
    assert!(mask.contains(iou::PollFlags::POLLIN));
    let mut buf = [0; MESSAGE.len()];
    read.read(&mut buf);
    assert_eq!(buf, MESSAGE);
    Ok(())
}

#[test]
fn test_poll_remove() -> io::Result<()> {
    let mut ring = iou::IoUring::new(2)?;
    let (read, _write) = net::UnixStream::pair()?;

    unsafe {
        let mut sqe = ring.next_sqe().expect("failed to get sqe");
        sqe.prep_poll_add(read.as_raw_fd(), iou::PollFlags::POLLIN);
        sqe.set_user_data(0xDEADBEEF);
        ring.submit_sqes()?;

        let mut sqe = ring.next_sqe().expect("failed to get sqe");
        sqe.prep_poll_remove(0xDEADBEEF);
        ring.submit_sqes()?;
        for _ in 0..2 {
            let cqe = ring.wait_for_cqe()?;
            let _ = cqe.result()?;
        }
        Ok(())
    }
}
