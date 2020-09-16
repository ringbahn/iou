use std::{
    io::{self, Read, Write},
    os::unix::{io::AsRawFd, net},
};
const MESSAGE: &'static [u8] = b"Hello World";

#[test]
fn test_poll_add() -> io::Result<()> {
    let mut ring = iou::IoUring::new(2)?;
    let (mut read, mut write) = net::UnixStream::pair()?;
    unsafe {
        let mut sqe = ring.prepare_sqe().expect("failed to get sqe");
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
    read.read(&mut buf)?;
    assert_eq!(buf, MESSAGE);
    Ok(())
}

#[test]
fn test_poll_remove() -> io::Result<()> {
    let mut ring = iou::IoUring::new(2)?;
    let (read, _write) = net::UnixStream::pair()?;
    let uname = nix::sys::utsname::uname();
    let version = semver::Version::parse(uname.release());
    unsafe {
        let mut sqe = ring.prepare_sqe().expect("failed to get sqe");
        sqe.prep_poll_add(read.as_raw_fd(), iou::PollFlags::POLLIN);
        sqe.set_user_data(0xDEADBEEF);
        ring.submit_sqes()?;

        let mut sqe = ring.prepare_sqe().expect("failed to get sqe");
        sqe.prep_poll_remove(0xDEADBEEF);
        sqe.set_user_data(42);
        ring.submit_sqes()?;
        for _ in 0..2 {
            let cqe = ring.wait_for_cqe()?;
            let user_data = cqe.user_data();
            if version < semver::Version::parse("5.5.0-0") {
                let _ = cqe.result()?;
            } else if user_data == 0xDEADBEEF {
                let err = cqe
                    .result()
                    .expect_err("on kernels >=5.5 error is expected");
                let err_no = nix::errno::Errno::from_i32(
                    err.raw_os_error()
                        .expect("on kernels >=5.5 os_error is expected"),
                );
                assert_eq!(err_no, nix::errno::Errno::ECANCELED);
            } else {
                let _ = cqe.result()?;
            }
        }
        Ok(())
    }
}
