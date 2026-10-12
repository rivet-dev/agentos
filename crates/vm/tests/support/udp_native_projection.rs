// Functional host-local controls: no internet, namespaces, or administrator setup.
fn receive_projection_probe(destination: Ipv4Addr, forced_nonloopback: bool, unavailable: bool) {
    let runtime =
        agentos_driver_tokio::TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .unwrap();
    let handle = runtime.handle();
    let receiver = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    receiver.set_nonblocking(true).unwrap();
    let port = receiver.local_addr().unwrap().port();
    let mut capture = if unavailable {
        // Same owner state as unsupported capture: no host metadata options
        // enabled, zero supported mask; ordinary payload delivery must survive.
        NativeUdpMetadataCapture {
            supported: 0,
            loopback_index: None,
            control: vec![0; CONTROL_BYTES],
        }
    } else {
        NativeUdpMetadataCapture::configure(&receiver, false).unwrap()
    };
    if !unavailable {
        assert_ne!(capture.supported & RECEIVE_PKTINFO_V4, 0);
    }
    if forced_nonloopback {
        // Inject only the trusted host classification input, not packet facts.
        // The actual loopback packet still supplies destination/spec_dst/index.
        capture.loopback_index = Some(u32::MAX);
    }
    let socket = {
        let _entered = handle.tokio_handle().enter();
        tokio::net::UdpSocket::from_std(receiver).unwrap()
    };
    let sender = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    sender.send_to(&[0, 255, 128], (destination, port)).unwrap();
    handle.tokio_handle().block_on(async {
        let observed_interface = if !forced_nonloopback && !unavailable {
            // Peek exactly the queued packet without consuming or changing its
            // metadata. Destination address alone does not identify ingress.
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    socket.readable().await.unwrap();
                    let peeked = socket.try_io(tokio::io::Interest::READABLE, || {
                        let mut bytes = [0; 3];
                        let mut iov = [IoSliceMut::new(&mut bytes)];
                        let mut control = vec![0; CONTROL_BYTES];
                        let message = recvmsg::<SockaddrStorage>(
                            socket.as_raw_fd(),
                            &mut iov,
                            Some(&mut control),
                            MsgFlags::MSG_DONTWAIT | MsgFlags::MSG_PEEK,
                        )
                        .map_err(io::Error::from)?;
                        assert!(!message.flags.contains(MsgFlags::MSG_CTRUNC));
                        let index = message
                            .cmsgs()
                            .map_err(io::Error::from)?
                            .find_map(|item| match item {
                                ControlMessageOwned::Ipv4PacketInfo(info) => {
                                    Some(info.ipi_ifindex as u32)
                                }
                                _ => None,
                            })
                            .expect("actual native packet-info ingress index");
                        assert_ne!(index, 0);
                        Ok(index)
                    });
                    match peeked {
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                        result => break Some(result.unwrap()),
                    }
                }
            })
            .await
            .expect("bounded same-packet metadata peek")
        } else {
            None
        };
        let (length, remote, metadata) =
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    socket.readable().await.unwrap();
                    let mut bytes = [0; 3];
                    match capture.receive(&socket, &mut bytes, RECEIVE_PKTINFO_V4) {
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                        result => {
                            let received = result.unwrap();
                            assert_eq!(bytes, [0, 255, 128]);
                            break received;
                        }
                    }
                }
            })
            .await
            .expect("bounded host-local UDP receive");
        assert_eq!(length, 3);
        assert_eq!(remote.port(), sender.local_addr().unwrap().port());
        if unavailable {
            assert!(metadata.is_none());
            return;
        }
        let facts = metadata.expect("requested supported packet facts");
        assert_eq!(facts.destination, IpAddr::V4(destination));
        assert_eq!(facts.ipv4_local_address, Some(destination));
        let expected =
            if observed_interface.is_some() && observed_interface == capture.loopback_index {
                GuestNetworkInterface::Loopback
            } else {
                GuestNetworkInterface::Ethernet
            };
        assert_eq!(facts.interface, expected);
        let expected_guest_index = match expected {
            GuestNetworkInterface::Loopback => 1,
            GuestNetworkInterface::Ethernet => 2,
        };
        assert_eq!(
            facts.interface as u32, expected_guest_index,
            "only the guest interface identity is exported"
        );
    });
}

#[test]
fn native_udp_injected_nonloopback_classification_preserves_packet_facts() {
    receive_projection_probe(Ipv4Addr::new(127, 0, 0, 2), true, false);
}

#[test]
fn native_udp_unavailable_capture_preserves_ordinary_datagram() {
    receive_projection_probe(Ipv4Addr::LOCALHOST, false, true);
}

#[test]
fn native_udp_local_interface_projects_only_guest_identity() {
    let address = nix::ifaddrs::getifaddrs().unwrap().find_map(|entry| {
        if !entry.flags.contains(nix::net::if_::InterfaceFlags::IFF_UP)
            || entry
                .flags
                .contains(nix::net::if_::InterfaceFlags::IFF_LOOPBACK)
        {
            return None;
        }
        let address = entry.address?.as_sockaddr_in()?.ip();
        (!address.is_loopback() && !address.is_unspecified()).then_some(address)
    });
    eprintln!("native_nonloopback_fixture={}", address.is_some());
    receive_projection_probe(address.unwrap_or(Ipv4Addr::LOCALHOST), false, false);
}
