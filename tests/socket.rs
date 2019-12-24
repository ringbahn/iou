use nix::sys::socket::{AddressFamily, SockProtocol, SockType, InetAddr, SockFlag};
use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::io::{AsRawFd, FromRawFd},
};

const MESSAGE: &'static [u8] = "Hello World".as_bytes();

#[test]
fn accept() -> io::Result<()> {
    let mut ring = iou::IoUring::new(1)?;

    let listener = TcpListener::bind(("0.0.0.0", 0))?;
    listener.set_nonblocking(true)?;

    let mut stream = TcpStream::connect(listener.local_addr()?)?;
    stream.write_all(MESSAGE)?;

    let fd = listener.as_raw_fd();
    let mut sq = ring.sq();
    let mut sqe = sq.next_sqe().expect("failed to get sqe");
    unsafe {
        sqe.prep_accept(fd, iou::SockFlag::empty());
        sq.submit()?;
    }
    let cqe = ring.wait_for_cqe()?;
    let accept_fd = cqe.result()?;
    let mut accept_buf = [0; MESSAGE.len()];
    let mut stream = unsafe { TcpStream::from_raw_fd(accept_fd as _) };
    stream.read_exact(&mut accept_buf)?;
    assert_eq!(accept_buf, MESSAGE);
    Ok(())
}

#[test]
fn connect() -> io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", 0))?;
    listener.set_nonblocking(true)?;
    let listener_addr = iou::SockAddr::new_inet(InetAddr::from_std(&listener.local_addr()?));

    let socket = nix::sys::socket::socket(
        AddressFamily::Inet,
        SockType::Stream,
        SockFlag::SOCK_NONBLOCK,
        SockProtocol::Tcp,
    )
    .map_err(|_| io::Error::new(io::ErrorKind::Other, "failed to create socket"))?;

    let mut ring = iou::IoUring::new(1)?;
    let mut sqe = ring.next_sqe().expect("failed to get sqe");
    unsafe {
        sqe.prep_connect(socket, &listener_addr);
        sqe.set_user_data(42);
        ring.submit_sqes()?;
    }
    let cqe = ring.wait_for_cqe()?;
    let _res = cqe.result()?;
    assert_eq!(cqe.user_data(), 42);
    Ok(())
}
