use nix::sys::socket::{AddressFamily, SockProtocol, SockType, InetAddr, SockFlag};
use std::{io, net::TcpListener};

#[test]
#[ignore] // kernel 5.5 needed for connect
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
    let mut sqe = ring.prepare_sqe().expect("failed to get sqe");
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
