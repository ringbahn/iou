use std::fs::File;
use std::io::{self, IoSliceMut};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

const TEXT: &[u8] = b"I really wanna stop
But I just gotta taste for it
I feel like I could fly with the ball on the moon
So honey hold my hand you like making me wait for it
I feel like I could die walking up to the room, oh yeah

Late night watching television
But how we get in this position?
It's way too soon, I know this isn't love
But I need to tell you something

I really really really really really really like you";

#[test]
fn vectored_read_test() -> io::Result<()> {
    let mut io_uring = iou::IoUring::new(32)?;

    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("text.txt");
    let file = File::open(&path)?;
    let mut buf1 = [0; 4096];
    let mut bufs = [IoSliceMut::new(&mut buf1)];

    unsafe {
        let mut sq = io_uring.sq();
        let mut sqe = sq.prepare_sqe().unwrap();
        sqe.prep_read_vectored(file.as_raw_fd(), &mut bufs[..], 0);
        sqe.set_user_data(0xDEADBEEF);
        sq.submit()?;
    }

    let n = {
        let mut cq = io_uring.cq();
        let cqe = cq.wait_for_cqe()?;
        assert_eq!(cqe.user_data(), 0xDEADBEEF);
        cqe.result()? as usize
    };

    assert_eq!(&TEXT[..n], &buf1[..n]);
    Ok(())
}

#[test]
fn read_test() -> io::Result<()> {
    let mut io_uring = iou::IoUring::new(32)?;

    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("text.txt");
    let file = File::open(&path)?;
    let mut buf = [0; 4096];

    unsafe {
        let mut sq = io_uring.sq();
        let mut sqe = sq.prepare_sqe().unwrap();
        sqe.prep_read(file.as_raw_fd(), &mut buf[..], 0);
        sqe.set_user_data(0xDEADBEEF);
        sq.submit()?;
    }

    let n = {
        let mut cq = io_uring.cq();
        let cqe = cq.wait_for_cqe()?;
        assert_eq!(cqe.user_data(), 0xDEADBEEF);
        cqe.result()? as usize
    };

    assert_eq!(&TEXT[..n], &buf[..n]);
    Ok(())
}

#[test]
fn read_registered_buf() -> io::Result<()> {
    let mut io_uring = iou::IoUring::new(32)?;
    let bufs = vec![Box::new([0u8; 4096]) as Box<[u8]>];
    let mut buf: iou::registrar::RegisteredBuf = io_uring.registrar().register_buffers(bufs)?.next().unwrap();

    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("text.txt");
    let file = File::open(&path)?;

    unsafe {
        let mut sq = io_uring.sq();
        let mut sqe = sq.prepare_sqe().unwrap();
        sqe.prep_read(file.as_raw_fd(), buf.as_mut(), 0);
        sqe.set_user_data(0xDEADBEEF);
        assert!(sqe.raw().opcode == uring_sys::IoRingOp::IORING_OP_READ_FIXED as u8);
        sq.submit()?;
    }

    let n = {
        let mut cq = io_uring.cq();
        let cqe = cq.wait_for_cqe()?;
        assert_eq!(cqe.user_data(), 0xDEADBEEF);
        cqe.result()? as usize
    };

    assert_eq!(&TEXT[..n], &buf.slice_to(n)[..]);
    Ok(())
}
