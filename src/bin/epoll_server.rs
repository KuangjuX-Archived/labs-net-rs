
use nix::sys::epoll::*;
use nix::sys::socket::*;

fn main() {
    let socket_fd = socket(
        AddressFamily::Inet, 
        SockType::Datagram, 
        SockFlag::SOCK_NONBLOCK, 
        SockProtocol::Tcp
    ).expect("Fail to new socket");
}