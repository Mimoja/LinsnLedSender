use pnet::{
    packet::ethernet::{EtherType, MutableEthernetPacket},
    util::MacAddr,
};

pub const HEADER_SIZE: usize = 0x20;
pub const ETHERNET_TYPE_SENDER: u16 = 0xAA55_u16;
pub const PAYLOAD_SIZE_SENDER: usize = 1440;

pub const ETHERNET_TYPE_RECEIVER: u16 = 0xAA56_u16;
pub const PAYLOAD_SIZE_RECEIVER: usize = 1450;

pub const LINSN_FRAME_WIDTH: u32 = 1024;
pub const LINSN_FRAME_HEIGHT: u32 = 512;

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
pub enum LinsnCommand {
    NONE = 0x00u8,
    CONFIG = 0x61u8,
    ANNOUNCE = 0x96u8,
}

pub enum ColorFormat {
    RGB,
    GBR,
    BRG,
    BGR,
}

pub fn pixel_to_bytes(format: ColorFormat, pixel: &image::Rgb<u8>) -> [u8; 3] {
    match format {
        ColorFormat::GBR => [pixel[1], pixel[2], pixel[0]],
        ColorFormat::RGB => [pixel[0], pixel[1], pixel[2]],
        ColorFormat::BRG => [pixel[2], pixel[0], pixel[1]],
        ColorFormat::BGR => [pixel[2], pixel[1], pixel[0]],
    }
}

pub fn rgb_to_bytes(format: ColorFormat, red: u16, green: u16, blue: u16) -> [u8; 3] {
    let r = (red & 0xFF) as u8;
    let g = (green & 0xFF) as u8;
    let b = (blue & 0xFF) as u8;

    match format {
        ColorFormat::GBR => [g, b, r],
        ColorFormat::RGB => [r, g, b],
        ColorFormat::BRG => [b, r, g],
        ColorFormat::BGR => [b, g, r],
    }
}

#[derive(Debug, Copy, Clone)]
pub struct LinsnSenderPacket {
    pub header: LinsnHeader,
    pub payload: [u8; PAYLOAD_SIZE_SENDER],
}

#[derive(Debug, Copy, Clone)]
pub struct LinsnReceiverPacket {
    pub header: LinsnHeader,
    pub payload: [u8; PAYLOAD_SIZE_RECEIVER],
}

#[derive(Debug, Copy, Clone)]
pub struct LinsnHeader {
    pub package_id: u32,
    pub unknown: [u8; 4],
    pub cmd: u8,
    pub cmd_data: [u8; 22],
    pub checksum: u8,
}

impl LinsnHeader {
    pub fn new(package_id: u32, cmd: u8, cmd_data: [u8; 22]) -> Self {
        let unknown = [0u8; 4];

        LinsnHeader {
            package_id,
            unknown,
            cmd,
            cmd_data,
            checksum: LinsnHeader::calculate_checksum(unknown, cmd, cmd_data),
        }
    }

    fn calculate_checksum(unknown: [u8; 4], cmd: u8, cmd_data: [u8; 22]) -> u8 {
        let mut checksum: u32 = 0;
        for i in unknown {
            checksum += i as u32;
        }
        checksum += cmd as u32;
        for i in cmd_data {
            checksum += i as u32;
        }

        (0x100 - (checksum & 0xFF)) as u8
    }

    pub fn to_bytes(self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend_from_slice(&self.package_id.to_le_bytes());
        bytes.extend_from_slice(&self.unknown);
        bytes.push(self.cmd);
        bytes.extend_from_slice(&self.cmd_data);
        bytes.push(self.checksum);
        bytes
    }

    pub fn empty(package_id: u32) -> Self {
        LinsnHeader {
            package_id,
            unknown: [0u8; 4],
            cmd: LinsnCommand::NONE as u8,
            cmd_data: [0u8; 22],
            checksum: 0,
        }
    }

    pub fn identify(package_id: u32, sender_mac: MacAddr) -> Self {
        LinsnHeader::new(
            package_id,
            LinsnCommand::ANNOUNCE as u8,
            [
                0x00,
                0x00,
                0x00,
                0x85,
                0x1f,
                0xff,
                0xff,
                0xff,
                0xff,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                sender_mac.0,
                sender_mac.1,
                sender_mac.2,
                sender_mac.3,
                sender_mac.4,
                sender_mac.5,
            ],
        )
    }
    pub fn chunk_start(sender_mac: MacAddr) -> Self {
        LinsnHeader::identify(0, sender_mac)
    }
}

impl LinsnSenderPacket {
    pub fn to_bytes(self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend_from_slice(&self.header.to_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }
    pub fn as_ethernet(&self, src: Option<MacAddr>, dst: Option<MacAddr>) -> MutableEthernetPacket {
        let src_mac = match src {
            Some(mac) => mac,
            None => MacAddr::broadcast(),
        };
        let dst_mac = match dst {
            Some(mac) => mac,
            None => MacAddr::zero(),
        };
        let ethernet_payload = self.to_bytes();

        let packet_size = MutableEthernetPacket::minimum_packet_size() + ethernet_payload.len();
        let packet_data = vec![0u8; packet_size];
        let mut ethernet_packet = MutableEthernetPacket::owned(packet_data).unwrap();

        ethernet_packet.set_source(src_mac);
        ethernet_packet.set_destination(dst_mac);
        ethernet_packet.set_ethertype(EtherType(ETHERNET_TYPE_SENDER));

        ethernet_packet.set_payload(&ethernet_payload);

        ethernet_packet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checksum_in_new() {
        let package_id = 0x12u32;
        let cmd = 0x34u8;
        let cmd_data: [u8; 22] = [0x01u8; 22];

        let expected_checksum = 0xb6;

        let header = LinsnHeader::new(package_id, cmd, cmd_data);

        assert_eq!(
            header.checksum, expected_checksum,
            "Checksum in new() does not match expected value"
        );
    }

    #[test]
    fn test_to_bytes_function() {
        let package_id = 0x12345678u32;
        let unknown: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
        let cmd = 0xAAu8;
        let cmd_data: [u8; 22] = [
            0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xA0, 0xB0, 0xC0, 0xD0, 0xE0,
            0xF0, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
        ];
        let chk_sum = 0xbau8;

        let expected_bytes: [u8; 32] = [
            package_id.to_le_bytes()[0],
            package_id.to_le_bytes()[1],
            package_id.to_le_bytes()[2],
            package_id.to_le_bytes()[3],
            unknown[0],
            unknown[1],
            unknown[2],
            unknown[3],
            cmd,
            cmd_data[0],
            cmd_data[1],
            cmd_data[2],
            cmd_data[3],
            cmd_data[4],
            cmd_data[5],
            cmd_data[6],
            cmd_data[7],
            cmd_data[8],
            cmd_data[9],
            cmd_data[10],
            cmd_data[11],
            cmd_data[12],
            cmd_data[13],
            cmd_data[14],
            cmd_data[15],
            cmd_data[16],
            cmd_data[17],
            cmd_data[18],
            cmd_data[19],
            cmd_data[20],
            cmd_data[21],
            chk_sum,
        ];

        let checksum = LinsnHeader::calculate_checksum(unknown, cmd, cmd_data);
        let header = LinsnHeader {
            package_id,
            unknown,
            cmd,
            cmd_data,
            checksum,
        };
        let bytes = header.to_bytes();

        assert_eq!(
            bytes, expected_bytes,
            "Byte representation does not match expected value"
        );
    }
}
