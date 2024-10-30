use crate::linsn::{
    pixel_to_bytes, ColorFormat, LinsnHeader, LinsnSenderPacket, Pixel, PAYLOAD_SIZE_SENDER,
};
use pnet::datalink;
use pnet::datalink::Channel;
use pnet::datalink::DataLinkSender;
use pnet::packet::Packet;
use pnet::util::MacAddr;
use std::sync::{Arc, Mutex};

const BYTES_PER_PIXEL: usize = 3;
const CHUNK_SIZE: usize = PAYLOAD_SIZE_SENDER / BYTES_PER_PIXEL;

pub trait LinsnSocket {
    fn send(self, image: Vec<Pixel>, dst_mac: MacAddr);
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
    fn send(self, image: Vec<Pixel>, dst_mac: MacAddr) {
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
    }
}
