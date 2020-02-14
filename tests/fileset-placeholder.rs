use iou::{IoUring, RegisteredFd, SubmissionFlags};
use std::fs::{self, File};
use std::io::{IoSlice, Read};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::Duration;

const TEXT: &[u8] = b"hello there\n";

#[test]
#[ignore] // fails if sparse filesets aren't supported
fn main() -> std::io::Result<()> {
    let mut ring = IoUring::new(2)?;
    let mut registrar = ring.registrar();

    let reserve_files = [RegisteredFd::placeholder().as_fd(), 4096];
    registrar.register_files(&reserve_files)?;
    assert!(registrar.fileset().iter().all(|fd| fd.is_placeholder()));

    println!("makes it here");

    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("tmp-fileset-placeholder.txt");

    let file = std::fs::File::create(&path)?;
    let fd_slice = &[file.as_raw_fd()];

    // update a random fileset entry
    let offset = 713;

    let reg_file = registrar.fileset()[offset];

    registrar.update_registered_files(offset, fd_slice)?;
    assert!(!registrar.fileset()[offset].is_placeholder());

    let reg_file = registrar.fileset()[offset];

    let bufs = &[IoSlice::new(&TEXT)];

    let mut sqe = ring.next_sqe().unwrap();

    unsafe {
        sqe.prep_write_vectored(reg_file, bufs, 0);
        sqe.set_user_data(0xDEADBEEF);
    }

    ring.submit_sqes()?;

    let cqe = ring.wait_for_cqe()?;
    assert_eq!(cqe.user_data(), 0xDEADBEEF);

    let n = cqe.result()?;
    assert!(n == TEXT.len());

    let mut file = File::open(&path)?;
    let mut buf = vec![];
    file.read_to_end(&mut buf)?;
    assert_eq!(&TEXT[..n], &buf[..n]);

    Ok(())
}
