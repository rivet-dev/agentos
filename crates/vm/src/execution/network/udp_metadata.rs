//! Fixed-size native UDP packet facts. Host interface identities stop here.
#[cfg(target_os = "linux")]
use agentos_vm_kernel::socket_table::GuestNetworkInterface;
use agentos_vm_kernel::socket_table::{DatagramMetadata, DatagramReceiveOptions};
#[cfg(target_os = "linux")]
use nix::libc;
#[cfg(target_os = "linux")]
use nix::sys::socket::{
    recvmsg, setsockopt, sockopt, ControlMessageOwned, MsgFlags, SockaddrStorage,
};
use std::io;
#[cfg(target_os = "linux")]
use std::io::IoSliceMut;
use std::net::SocketAddr;
#[cfg(target_os = "linux")]
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;

pub(super) const RECEIVE_PKTINFO_V4: u8 = DatagramReceiveOptions::PACKET_INFO_V4;
pub(super) const RECEIVE_TOS_V4: u8 = DatagramReceiveOptions::TYPE_OF_SERVICE_V4;
pub(super) const RECEIVE_PKTINFO_V6: u8 = DatagramReceiveOptions::PACKET_INFO_V6;
pub(super) const RECEIVE_TCLASS_V6: u8 = DatagramReceiveOptions::TRAFFIC_CLASS_V6;
// Linux's supported mapped-v4 maximum is three records (96 bytes on 64-bit).
// Fixed build bound; extending supported controls must update this bound/tests.
#[cfg(target_os = "linux")]
const CONTROL_BYTES: usize = 128;

#[derive(Debug)]
pub(super) struct NativeUdpMetadataCapture {
    pub supported: u8,
    #[cfg(target_os = "linux")]
    loopback_index: Option<u32>,
    #[cfg(target_os = "linux")]
    control: Vec<u8>,
}

#[cfg(target_os = "linux")]
impl NativeUdpMetadataCapture {
    pub fn configure(socket: &std::net::UdpSocket, ipv6: bool) -> io::Result<Self> {
        let loopback_index = match nix::net::if_::if_nametoindex("lo") {
            Ok(index) => Some(index),
            Err(error) => {
                eprintln!(
                    "ERR_AGENTOS_UDP_METADATA_UNAVAILABLE: guest interface projection unavailable: {error}"
                );
                None
            }
        };
        let mut supported = 0;
        if loopback_index.is_some() {
            for bit in [
                RECEIVE_PKTINFO_V4,
                RECEIVE_TOS_V4,
                RECEIVE_PKTINFO_V6,
                RECEIVE_TCLASS_V6,
            ] {
                if !ipv6 && matches!(bit, RECEIVE_PKTINFO_V6 | RECEIVE_TCLASS_V6) {
                    continue;
                }
                let result = match bit {
                    RECEIVE_PKTINFO_V4 => {
                        setsockopt(socket, sockopt::Ipv4PacketInfo, &true).map_err(io::Error::from)
                    }
                    RECEIVE_TOS_V4 => {
                        setsockopt(socket, sockopt::IpRecvTos, &true).map_err(io::Error::from)
                    }
                    RECEIVE_PKTINFO_V6 => setsockopt(socket, sockopt::Ipv6RecvPacketInfo, &true)
                        .map_err(io::Error::from),
                    RECEIVE_TCLASS_V6 => {
                        setsockopt(socket, sockopt::Ipv6RecvTClass, &true).map_err(io::Error::from)
                    }
                    _ => unreachable!(),
                };
                match result {
                    Ok(()) => supported |= bit,
                    Err(error)
                        if matches!(
                            error.raw_os_error(),
                            Some(libc::ENOPROTOOPT | libc::EOPNOTSUPP)
                        ) =>
                    {
                        eprintln!(
                            "ERR_AGENTOS_UDP_METADATA_UNAVAILABLE: receive option bit {bit} unavailable: {error}"
                        );
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        // Traffic-class delivery also needs packet-family/destination facts.
        // Missing optional host support must not break unrelated UDP receives.
        if supported & RECEIVE_PKTINFO_V4 == 0 {
            supported &= !RECEIVE_TOS_V4;
        }
        if supported & RECEIVE_PKTINFO_V6 == 0 {
            supported &= !RECEIVE_TCLASS_V6;
        }
        Ok(Self {
            supported,
            loopback_index,
            control: vec![0; CONTROL_BYTES],
        })
    }

    pub fn receive(
        &mut self,
        socket: &tokio::net::UdpSocket,
        data: &mut [u8],
        receive_options: u8,
    ) -> io::Result<(usize, SocketAddr, Option<DatagramMetadata>)> {
        socket.try_io(tokio::io::Interest::READABLE, || {
            // One bounded control allocation belongs to this native owner and is
            // reused for every datagram; supported typed controls allocate nothing.
            let mut iov = [IoSliceMut::new(data)];
            let message = recvmsg::<SockaddrStorage>(socket.as_raw_fd(), &mut iov,
                Some(&mut self.control), MsgFlags::MSG_DONTWAIT).map_err(io::Error::from)?;
            if message.flags.contains(MsgFlags::MSG_CTRUNC) {
                return Err(metadata_error("native control exceeds fixed 128-byte capture bound; extend the build-time UDP metadata bound"));
            }
            let received = message.bytes;
            let source = message.address.ok_or_else(|| metadata_error("missing native source address"))?;
            let remote = if let Some(address) = source.as_sockaddr_in() {
                SocketAddr::V4((*address).into())
            } else if let Some(address) = source.as_sockaddr_in6() {
                SocketAddr::V6((*address).into())
            } else { return Err(metadata_error("unsupported source family")); };
            if self.supported == 0 { return Ok((received, remote, None)); }
            let mut v4 = None;
            let mut v6 = None;
            let mut tos = 0;
            let mut tclass = 0;
            for control in message.cmsgs().map_err(|error| metadata_error(&format!("malformed native control: {error}")))? {
                match control {
                    ControlMessageOwned::Ipv4PacketInfo(info) => {
                        v4 = Some((Ipv4Addr::from(info.ipi_addr.s_addr.to_ne_bytes()),
                            Ipv4Addr::from(info.ipi_spec_dst.s_addr.to_ne_bytes()), info.ipi_ifindex as u32));
                    }
                    ControlMessageOwned::Ipv6PacketInfo(info) => {
                        v6 = Some((Ipv6Addr::from(info.ipi6_addr.s6_addr), info.ipi6_ifindex));
                    }
                    ControlMessageOwned::Ipv4Tos(value) => tos = value,
                    ControlMessageOwned::Ipv6TClass(value) => {
                        tclass = u8::try_from(value).map_err(|_| metadata_error("invalid native traffic class"))?;
                    }
                    _ => return Err(metadata_error("unexpected native receive control")),
                }
            }
            let (destination, local, index, traffic_class) = if let Some((dst, local, index)) = v4 {
                (IpAddr::V4(dst), Some(local), index, tos)
            } else if let Some((dst, index)) = v6 {
                if let Some(ipv4) = dst.to_ipv4_mapped() { (IpAddr::V4(ipv4), None, index, tos) }
                else { (IpAddr::V6(dst), None, index, tclass) }
            } else { return Ok((received as usize, remote, None)); };
            if index == 0 { return Err(metadata_error("native interface identity is unavailable")); }
            let interface = if Some(index) == self.loopback_index { GuestNetworkInterface::Loopback } else { GuestNetworkInterface::Ethernet };
            Ok((received as usize, remote, Some(DatagramMetadata { destination, ipv4_local_address: if receive_options & (RECEIVE_PKTINFO_V4 | RECEIVE_PKTINFO_V6) != 0 { local } else { None }, interface, traffic_class })))
        })
    }
}

#[cfg(target_os = "linux")]
fn metadata_error(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("ERR_AGENTOS_UDP_METADATA: {message}"),
    )
}

// Other published hosts retain ordinary UDP. Unsupported ancillary requests are
// rejected by the canonical supported-mask check before enabling the option.
#[cfg(not(target_os = "linux"))]
impl NativeUdpMetadataCapture {
    pub fn configure(_socket: &std::net::UdpSocket, _ipv6: bool) -> io::Result<Self> {
        eprintln!(
            "ERR_AGENTOS_UDP_METADATA_UNAVAILABLE: native ancillary capture is unavailable on this host; ordinary UDP remains available"
        );
        Ok(Self { supported: 0 })
    }

    pub fn receive(
        &mut self,
        socket: &tokio::net::UdpSocket,
        data: &mut [u8],
        _receive_options: u8,
    ) -> io::Result<(usize, SocketAddr, Option<DatagramMetadata>)> {
        socket
            .try_recv_from(data)
            .map(|(length, source)| (length, source, None))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod native_projection_tests {
    use super::*;
    include!("../../../tests/support/udp_native_projection.rs");
}
