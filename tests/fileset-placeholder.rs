use iou::{registrar::RegisteredFd, IoUring};
use std::fs::File;
use std::io::{IoSlice, Read};
use std::os::unix::io::AsRawFd;

const TEXT: &[u8] = b"hello there\n";

#[test]
fn main() -> std::io::Result<()> {
    let mut ring = IoUring::new(2)?;
    let mut registrar = ring.registrar();

    let reserve_files = [iou::registrar::PLACEHOLDER_FD; 1024];
    let fileset: Vec<RegisteredFd> = registrar.register_files(&reserve_files)?.collect();
    assert!(fileset.iter().all(|fd| fd.is_placeholder()));

    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("props");
    path.push("tmp-fileset-placeholder.txt");

    // update a random fileset entry with a valid file
    let file = std::fs::File::create(&path)?;
    let reg_file = registrar
        .update_registered_files(713, &[file.as_raw_fd()])?
        .collect::<Vec<_>>()[0];
    assert!(!reg_file.is_placeholder());

    let bufs = &[IoSlice::new(&TEXT)];
    let mut sqe = ring.prepare_sqe().unwrap();

    unsafe {
        sqe.prep_write_vectored(reg_file, bufs, 0);
        sqe.set_user_data(0xDEADBEEF);
    }

    ring.submit_sqes()?;
    let cqe = ring.wait_for_cqe()?;
    assert_eq!(cqe.user_data(), 0xDEADBEEF);

    let n = cqe.result()? as usize;
    assert!(n == TEXT.len());

    let mut file = File::open(&path)?;
    let mut buf = vec![];
    file.read_to_end(&mut buf)?;
    assert_eq!(&TEXT[..n], &buf[..n]);

    Ok(())
}
