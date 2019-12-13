#![feature(test)]
extern crate libc;
extern crate test;

use std::{io, os::unix::io::RawFd};

pub fn pipe() -> io::Result<(RawFd, RawFd)> {
    unsafe {
        let mut fds = core::mem::MaybeUninit::<[libc::c_int; 2]>::uninit();

        let res = libc::pipe(fds.as_mut_ptr() as *mut libc::c_int);

        if res < 0 {
            Err(io::Error::from_raw_os_error(-res))
        } else {
            Ok((fds.assume_init()[0], fds.assume_init()[1]))
        }
    }
}

#[test]
fn test_poll_add() -> io::Result<()> {
    let mut ring = iou::IoUring::new(2)?;
    let (read, write) = pipe()?;

    unsafe {
        let mut sqe = ring.next_sqe().expect("no sqe");
        sqe.prep_poll_add(read, iou::PollMask::POLLIN);
        sqe.set_user_data(0xDEADBEEF);
        ring.submit_sqes()?;
    }

    let res = unsafe {
        let buf = b"hello";
        libc::write(
            write,
            buf.as_ptr() as *const libc::c_void,
            buf.len() as libc::size_t,
        )
    };

    if res < 0 {
        return Err(io::Error::from_raw_os_error(-res as _));
    }

    let cqe = ring.wait_for_cqe()?;
    assert_eq!(cqe.user_data(), 0xDEADBEEF);
    let mask = unsafe { iou::PollMask::from_bits_unchecked(cqe.result()? as _) };
    assert!(mask.contains(iou::PollMask::POLLIN));
    unsafe {
        libc::close(write);
        libc::close(read);
    }
    Ok(())
}

#[test]
fn test_poll_remove() -> io::Result<()> {
    let mut ring = iou::IoUring::new(2)?;
    let (read, write) = pipe()?;

    unsafe {
        let mut sqe = ring.next_sqe().expect("no sqe");
        sqe.prep_poll_add(read, iou::PollMask::POLLIN);
        sqe.set_user_data(0xDEADBEEF);
        ring.submit_sqes()?;

        let mut sqe = ring.next_sqe().expect("no sqe");
        sqe.prep_poll_remove(0xDEADBEEF);
        ring.submit_sqes()?;
        for _ in 0..2 {
            let cqe = ring.wait_for_cqe()?;
            let _ = cqe.result()?;
        }
        libc::close(write);
        libc::close(read);
        Ok(())
    }
}
