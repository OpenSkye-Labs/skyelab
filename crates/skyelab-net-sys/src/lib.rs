//! `SO_EXCLUSIVEADDRUSE` for a local listener on Windows. Without it a socket with `SO_REUSEADDR` (any process of
//! the same user) can bind the same port over ours and take its connections. Neither std nor socket2 sets it, so the
//! one `setsockopt` lives here; socket2 makes, binds and listens. Windows only.
#![allow(unsafe_code)]

#[cfg(windows)]
pub use win::bind_exclusive;

#[cfg(windows)]
mod win {
    use std::io;
    use std::net::{SocketAddr, TcpListener};
    use std::os::windows::io::AsRawSocket as _;

    use socket2::{Domain, Protocol, Socket, Type};
    use windows_sys::Win32::Networking::WinSock::{
        SO_EXCLUSIVEADDRUSE, SOCKET, SOCKET_ERROR, SOL_SOCKET, setsockopt,
    };

    /// A TCP listener on `addr` that no other socket can bind over while it is open (not inheritable, as socket2
    /// makes every socket).
    pub fn bind_exclusive(addr: SocketAddr) -> io::Result<TcpListener> {
        let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
        let on: i32 = 1;
        // SAFETY: `socket` is an open socket this function owns for the whole call; `optval` points at `on`, a live
        // i32, and `optlen` is its size, as SO_EXCLUSIVEADDRUSE (a BOOL) expects. Set before `bind`, as it must be.
        let rc = unsafe {
            setsockopt(
                socket.as_raw_socket() as SOCKET,
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                (&raw const on).cast(),
                size_of::<i32>() as i32,
            )
        };
        if rc == SOCKET_ERROR {
            return Err(io::Error::last_os_error());
        }
        socket.bind(&addr.into())?;
        socket.listen(128)?;
        Ok(socket.into())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::net::Ipv4Addr;

        /// What a same-user process would try: SO_REUSEADDR, then bind the port (on loopback or the wildcard).
        fn hijack(addr: SocketAddr) -> io::Result<Socket> {
            let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
            socket.set_reuse_address(true)?;
            socket.bind(&addr.into())?;
            Ok(socket)
        }

        #[test]
        fn no_socket_can_bind_over_an_exclusive_listener() {
            let ours = bind_exclusive((Ipv4Addr::LOCALHOST, 0).into()).expect("bind");
            let addr = ours.local_addr().expect("addr");
            assert!(hijack(addr).is_err(), "bound over 127.0.0.1");
            assert!(
                hijack((Ipv4Addr::UNSPECIFIED, addr.port()).into()).is_err(),
                "bound over the wildcard"
            );
        }

        /// The control: without the option the same hijack succeeds, so the test above can fail.
        #[test]
        fn a_plain_listener_can_be_bound_over() {
            let ours = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
            let addr = ours.local_addr().expect("addr");
            assert!(hijack(addr).is_ok());
        }
    }
}
