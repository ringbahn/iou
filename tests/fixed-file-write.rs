use iou::{IoUring, RegisteredFd, Registrar};
use std::fs::{self, File};
use std::io::{IoSlice, Read};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

const TEXT: &[u8] = b"hello there";

#[test]
fn read_fixed() -> std::io::Result<()> {
    let mut ring = IoUring::new(2)?;

    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("tmp.txt");

    let file = File::create(&path)?;

    let fixed_fd = file.as_raw_fd();
    let mut reg: Registrar = ring.registrar();

    // register a new file
    let fileset: Vec<RegisteredFd> = reg.register_files(&[fixed_fd])?.collect();

    let bufs = &[IoSlice::new(&TEXT)];
    let reg_file = fileset[0];

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

    let _ = fs::remove_file(&path);

    Ok(())
}
