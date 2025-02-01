use crate::linsn::HEADER_SIZE;
use crate::linsn::{
    pixel_to_bytes, ColorFormat, LinsnHeader, LinsnSenderPacket, PAYLOAD_SIZE_SENDER,
};
use image::Rgb;
use pnet::datalink;
use pnet::datalink::Channel;
use pnet::datalink::DataLinkSender;
use pnet::packet::ethernet::MutableEthernetPacket;
use pnet::packet::Packet;
use pnet::util::MacAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use libc::{
    c_void, close, if_nametoindex, iovec, mmsghdr, sendmmsg, sockaddr_ll, socket, AF_PACKET,
    ETH_ALEN, ETH_P_ALL, SOCK_RAW,
};
use std::ffi::CString;
use std::mem;
use std::ptr;

const BYTES_PER_PIXEL: usize = 3;
const CHUNK_SIZE: usize = PAYLOAD_SIZE_SENDER / BYTES_PER_PIXEL;
pub trait LinsnSocket {
    fn send(&self, image: &Vec<Rgb<u8>>, dst_mac: MacAddr);
}

#[derive(Clone)]
pub struct SimpleSocketSender {
    tx: Arc<Mutex<Box<dyn DataLinkSender>>>,
    src_mac: MacAddr,
}

impl SimpleSocketSender {
    pub fn new(interface_name: &str) -> Self {
        let interfaces = datalink::interfaces();
        let interface = interfaces
            .into_iter()
            .find(|iface| iface.name == interface_name)
            .expect("Network interface not found");

        let channel: datalink::Channel = datalink::channel(&interface, Default::default()).unwrap();
        let tx = match channel {
            Channel::Ethernet(tx, _) => tx,
            _ => panic!("Unhandled channel type"),
        };
        Self {
            src_mac: interface.mac.unwrap_or(MacAddr::broadcast()),
            tx: Arc::new(Mutex::new(tx)),
        }
    }
}

impl LinsnSocket for SimpleSocketSender {
    fn send(&self, image: &Vec<Rgb<u8>>, dst_mac: MacAddr) {
        let before = Instant::now();
        let tx = Arc::clone(&self.tx);

        // Lock the transmitter to send the image
        let mut tx = tx.lock().expect("Failed to acquire lock on transmitter");
        let chunks = image.chunks(CHUNK_SIZE);
        for (package_id, chunk) in chunks.enumerate() {
            // Convert the pixel data to bytes
            let mut payload = vec![0_u8; PAYLOAD_SIZE_SENDER];
            for (index, pixel) in chunk.iter().enumerate() {
                let pbytes = pixel_to_bytes(ColorFormat::BRG, pixel);
                payload[index * BYTES_PER_PIXEL..(index + 1) * BYTES_PER_PIXEL]
                    .copy_from_slice(&pbytes);
            }

            // Create the whole payload
            let packet = LinsnSenderPacket {
                header: match package_id {
                    0 => LinsnHeader::chunk_start(self.src_mac),
                    _ => LinsnHeader::empty(package_id as u32),
                },
                payload: payload.try_into().unwrap(),
            };
            let ethernet_packet = packet.as_ethernet(Some(self.src_mac), Some(dst_mac));
            match tx.send_to(ethernet_packet.packet(), None) {
                Some(Ok(_)) => (),
                Some(Err(e)) => eprintln!("Failed to send packet: {}", e),
                None => eprintln!("Failed to send packet: No response"),
            }
        }
        let now = Instant::now();
        println!("Time for sending: {:.0?}", (now - before));
    }
}

#[derive(Clone)]
pub struct BatchedSocketSender {
    if_index: u32,
    sockfd: i32,
    src_mac: MacAddr,
}

impl BatchedSocketSender {
    pub fn new(interface_name: &str) -> Self {
        let interfaces = datalink::interfaces();
        let interface = interfaces
            .into_iter()
            .find(|iface| iface.name == interface_name)
            .expect("Network interface not found");

        let src_mac: MacAddr = interface.mac.or(Some(MacAddr::broadcast())).unwrap();

        unsafe {
            let if_name = CString::new(interface_name).unwrap();
            let if_index = if_nametoindex(if_name.as_ptr());
            if if_index == 0 {
                panic!("Failed to get interface index");
            }

            let sockfd = socket(AF_PACKET, SOCK_RAW, (ETH_P_ALL as u16).to_be() as i32);
            if sockfd == -1 {
                panic!("Failed to create socket");
            }

            // Setting buffer size
            let mut n: libc::c_int = 0;
            let mut n_len = mem::size_of::<libc::c_int>() as libc::socklen_t;
            let ret = libc::getsockopt(
                sockfd,
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &mut n as *mut i32 as *mut libc::c_void,
                &mut n_len,
            );
            if ret != 0 {
                panic!("failed to get socket params");
            }
            println!("Send Buffer size: {:}", n);

            n = 1024 * 1024 * 1024;
            let ret = libc::setsockopt(
                sockfd,
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &n as *const _ as *const libc::c_void,
                mem::size_of_val(&n) as libc::socklen_t,
            );
            if ret != 0 {
                panic!("failed to get socket params");
            }

            println!("Send Buffer size: {:}", n);
            Self {
                if_index,
                sockfd,
                src_mac,
            }
        }
    }
}

impl LinsnSocket for BatchedSocketSender {
    fn send(&self, image: &Vec<Rgb<u8>>, dst_mac: MacAddr) {
        let before: Instant = Instant::now();

        let mut socket_address: sockaddr_ll = sockaddr_ll {
            sll_family: AF_PACKET as u16,
            sll_protocol: (ETH_P_ALL as u16).to_be(),
            sll_ifindex: self.if_index as i32,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: ETH_ALEN as u8,
            sll_addr: [
                dst_mac.0, dst_mac.1, dst_mac.2, dst_mac.3, dst_mac.4, dst_mac.5, 0, 0,
            ],
        };
        const MAX_CHUNK_COUNT: usize = 1100;
        unsafe {
            let chunks = image.chunks(CHUNK_SIZE);
            let chunk_count = chunks.len();
            if chunk_count > MAX_CHUNK_COUNT {
                panic!("Too many chunks!");
            }
            let mut iovecs = [mem::zeroed::<iovec>(); MAX_CHUNK_COUNT];
            let mut msgs = [mem::zeroed::<mmsghdr>(); MAX_CHUNK_COUNT];

            let mut ethernet_packets = [[0u8; MutableEthernetPacket::minimum_packet_size()
                + PAYLOAD_SIZE_SENDER
                + HEADER_SIZE]; MAX_CHUNK_COUNT];

            for (package_id, chunk) in chunks.enumerate() {
                // Convert the pixel data to bytes
                let mut payload = vec![0 as u8; PAYLOAD_SIZE_SENDER];
                for (index, pixel) in chunk.iter().enumerate() {
                    let pbytes = pixel_to_bytes(ColorFormat::BRG, pixel);
                    payload[index * BYTES_PER_PIXEL..(index + 1) * BYTES_PER_PIXEL]
                        .copy_from_slice(&pbytes);
                }

                // Create the whole payload
                let packet: LinsnSenderPacket = LinsnSenderPacket {
                    header: match package_id {
                        0 => LinsnHeader::chunk_start(self.src_mac),
                        _ => LinsnHeader::empty(package_id as u32),
                    },
                    payload: payload.try_into().unwrap(),
                };
                ethernet_packets[package_id].copy_from_slice(
                    &packet
                        .as_ethernet(Some(self.src_mac), Some(dst_mac))
                        .packet(),
                );

                // iovec points to the packet buffer
                iovecs[package_id].iov_base = ethernet_packets[package_id].as_ptr() as *mut c_void;
                iovecs[package_id].iov_len = ethernet_packets[package_id].len();

                // mmsghdr contains message headers
                msgs[package_id].msg_hdr.msg_iov = &mut iovecs[package_id];
                msgs[package_id].msg_hdr.msg_iovlen = 1;
                msgs[package_id].msg_hdr.msg_name =
                    &mut socket_address as *mut sockaddr_ll as *mut c_void;
                msgs[package_id].msg_hdr.msg_namelen = mem::size_of::<sockaddr_ll>() as u32;
                msgs[package_id].msg_hdr.msg_control = ptr::null_mut();
                msgs[package_id].msg_hdr.msg_controllen = 0;
                msgs[package_id].msg_hdr.msg_flags = 0;
                msgs[package_id].msg_len = 0;
            }
            // let before_sending = Instant::now();
            let ret = sendmmsg(
                self.sockfd,
                msgs.as_mut_ptr(),
                ethernet_packets.len() as u32,
                0,
            );

            if ret == -1 {
                eprintln!("Failed to send packets");
                close(self.sockfd);
                return;
            } else {
                let now = Instant::now();
                // // println!("Time for preparing: {:.0?}", before-before_sending);
                // // println!("Time for sending: {:.0?}" , now-before_sending);
                if (now - before).as_millis() > (1000 / 60) {
                    println!(
                        "Time for sending dropped below 60 fps: {:.0?}",
                        (now - before)
                    );
                }
            }
        }
    }
}
