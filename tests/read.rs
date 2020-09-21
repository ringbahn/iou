use std::fs::File;
use std::io::{self, IoSliceMut};
use std::os::unix::io::{AsRawFd, RawFd};
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
fn read_test() -> io::Result<()> {
    let mut io_uring = iou::IoUring::new(32)?;

    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("text.txt");
    let file = File::open(&path)?;
    let mut buf1 = [0; 4096];
    let mut bufs = [io::IoSliceMut::new(&mut buf1)];

    unsafe {
        prep(&mut io_uring, &mut bufs, file.as_raw_fd())?;
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

#[inline(never)]
unsafe fn prep(ring: &mut iou::IoUring, bufs: &mut [IoSliceMut], fd: RawFd) -> io::Result<()> {
    let mut sq = ring.sq();
    let mut sqe = sq.prepare_sqe().unwrap();
    sqe.prep_read_vectored(fd, bufs, 0);
    sqe.set_user_data(0xDEADBEEF);
    sq.submit()?;
    Ok(())
}
