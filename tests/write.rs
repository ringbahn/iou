use std::fs::{self, File};
use std::io::{self, Read};
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
fn vectored_write_test() -> io::Result<()> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("vectored.tmp");

    let _ = fs::remove_file(&path);

    let n = {
        let mut io_uring = iou::IoUring::new(32)?;
        let bufs = [io::IoSlice::new(TEXT)];

        let file = File::create(&path)?;
        unsafe {
            let mut sq = io_uring.sq();
            let mut sqe = sq.prepare_sqe().unwrap();
            sqe.prep_write_vectored(file.as_raw_fd(), &bufs, 0);
            sqe.set_user_data(0xDEADBEEF);
            io_uring.sq().submit()?;
        }

        let mut cq = io_uring.cq();
        let cqe = cq.wait_for_cqe()?;
        drop(bufs); // hold bufs until after io completes
        assert_eq!(cqe.user_data(), 0xDEADBEEF);
        cqe.result()? as usize
    };

    let mut file = File::open(&path)?;
    let mut buf = vec![];
    file.read_to_end(&mut buf)?;
    assert_eq!(&TEXT[..n], &buf[..n]);
    let _ = fs::remove_file(&path);

    Ok(())
}

#[test]
fn write_test() -> io::Result<()> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("text.tmp");

    let _ = fs::remove_file(&path);

    let n = {
        let mut io_uring = iou::IoUring::new(32)?;

        let file = File::create(&path)?;
        unsafe {
            let mut sq = io_uring.sq();
            let mut sqe = sq.prepare_sqe().unwrap();
            sqe.prep_write(file.as_raw_fd(), TEXT, 0);
            sqe.set_user_data(0xDEADBEEF);
            io_uring.sq().submit()?;
        }

        let mut cq = io_uring.cq();
        let cqe = cq.wait_for_cqe()?;
        assert_eq!(cqe.user_data(), 0xDEADBEEF);
        cqe.result()? as usize
    };

    let mut file = File::open(&path)?;
    let mut buf = vec![];
    file.read_to_end(&mut buf)?;
    assert_eq!(&TEXT[..n], &buf[..n]);
    let _ = fs::remove_file(&path);

    Ok(())
}

#[test]
fn write_registered_buf() -> io::Result<()> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("write_registered_buf.tmp");

    let _ = fs::remove_file(&path);

    let mut io_uring = iou::IoUring::new(32)?;
    let bufs = vec![Box::new([0u8; 4096]) as Box<[u8]>];
    let mut buf: iou::registrar::RegisteredBuf =
        io_uring.registrar().register_buffers(bufs)?.next().unwrap();

    buf.as_mut().slice_to_mut(TEXT.len()).copy_from_slice(TEXT);

    let n = {
        let file = File::create(&path)?;
        unsafe {
            let mut sq = io_uring.sq();
            let mut sqe = sq.prepare_sqe().unwrap();
            sqe.prep_write(file.as_raw_fd(), buf.slice_to(TEXT.len()), 0);
            assert!(sqe.raw().opcode == uring_sys::IoRingOp::IORING_OP_WRITE_FIXED as u8);
            sqe.set_user_data(0xDEADBEEF);
            io_uring.sq().submit()?;
        }

        let mut cq = io_uring.cq();
        let cqe = cq.wait_for_cqe()?;
        assert_eq!(cqe.user_data(), 0xDEADBEEF);
        cqe.result()? as usize
    };

    let mut file = File::open(&path)?;
    let mut buf = vec![];
    file.read_to_end(&mut buf)?;
    assert_eq!(&TEXT[..n], &buf[..n]);
    let _ = fs::remove_file(&path);

    Ok(())
}
