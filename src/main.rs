mod linsn;

use std::time::Duration;
use std::time::Instant;

use gstreamer_app::AppSink;
use gstreamer_app::AppSinkCallbacks;
use libc::{
    c_void, close, if_nametoindex, iovec, mmsghdr, sendmmsg, sockaddr_ll, socket, AF_PACKET,
    ETH_ALEN, ETH_P_ALL, SOCK_RAW,
};
use linsn::{
    pixel_to_bytes, ColorFormat, LinsnHeader, LinsnSenderPacket, Pixel, HEADER_SIZE,
    PAYLOAD_SIZE_SENDER,
};
use pnet::datalink;
use pnet::datalink::Channel;
use pnet::packet::ethernet::MutableEthernetPacket;
use pnet::packet::Packet;
use pnet::util::MacAddr;
use std::ffi::CString;
use std::mem;
use std::ptr;
use std::thread;

const BUFFER_X: usize = 200;
const BUFFER_Y: usize = 1024;
const PANEL_X: usize = 192;
const PANEL_Y: usize = 192;
const BYTES_PER_PIXEL: usize = 3;
const CHUNK_SIZE: usize = PAYLOAD_SIZE_SENDER / BYTES_PER_PIXEL;
const CHUNK_COUNT: usize = BUFFER_Y * (BUFFER_X) / (PAYLOAD_SIZE_SENDER / 3) + 1;
use gstreamer::prelude::*;
use gstreamer::{ElementFactory, Pipeline};
use std::sync::{Arc, Mutex};

fn init_screencapture() {
    gstreamer::init().expect("Failed to initialize GStreamer");

    // Create a GStreamer pipeline
    let pipeline = Pipeline::with_name("screen-capture");

    // Create PipeWire source element (for Wayland screen capture)
    let src = ElementFactory::make("pipewiresrc").build().unwrap();
    // Create other elements: videoconvert and appsink
    let convert = ElementFactory::make("videoconvert").build().unwrap();

    // Configure appsink properties to enable frame capture
    let sink = ElementFactory::make("appsink")
        .property("emit-signals", true)
        .property("sync", false)
        .build()
        .unwrap();

    // Add elements to the pipeline
    pipeline
        .add_many([&src, &convert, &sink])
        .expect("Failed to add elements to the pipeline");

    // Link elements in the pipeline
    gstreamer::Element::link_many([&src, &convert, &sink]).expect("Failed to link elements");

    // Frame buffer to store the captured frame
    let frame_buffer = Arc::new(Mutex::new(Vec::new()));

    // Connect the new-sample signal on the appsink to capture frames
    let frame_buffer_clone = Arc::clone(&frame_buffer);
    let appsink = sink
        .dynamic_cast::<AppSink>()
        .expect("Sink element is expected to be an AppSink");

    appsink.set_callbacks(
        AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                // Pull the sample
                let sample = appsink.pull_sample().unwrap();

                // Get the buffer from the sample
                let buffer = sample.buffer().expect("Failed to get buffer from sample");

                // Map the buffer to read frame data
                let map = buffer
                    .map_readable()
                    .expect("Failed to map buffer readable");

                // Lock and write data to the frame buffer
                let mut buf = frame_buffer_clone.lock().unwrap();
                buf.clear();
                buf.extend_from_slice(map.as_slice());
                println!("Captured frame of size: {} bytes", buf.len());

                // Return success to allow pipeline to continue
                Ok(gstreamer::FlowSuccess::Ok)
            })
            .build(),
    );

    // Start the pipeline
    pipeline
        .set_state(gstreamer::State::Playing)
        .expect("Unable to set pipeline to `Playing` state");
}

fn fill_image(image: &mut [Pixel], counter: i16, image_counter: u128) {
    let sine_input = image_counter as f64 / 100.0;
    let sine = sine_input.sin() * 0x69 as f64;
    const CHANGE_PER_LINE_X: f64 = 0xff as f64 / PANEL_X as f64;
    const CHANGE_PER_LINE_Y: f64 = 0xff as f64 / PANEL_Y as f64;

    for fake_x in 0..BUFFER_X + 1 {
        if fake_x == 0 {
            continue;
        }
        let x: usize = fake_x - 1;
        if x > PANEL_X {
            continue;
        }
        for y in 0..BUFFER_Y {
            if y > PANEL_Y {
                continue;
            }
            if y < 8 {
                if x / 8 % 2 == 0 {
                    image[fake_x * BUFFER_Y + y] = Pixel::white()
                } else {
                    image[fake_x * BUFFER_Y + y] = Pixel::black()
                }
                continue;
            }

            image[fake_x * BUFFER_Y + y] = Pixel {
                r: (sine + 128.0 + 20.0) as u32 as u8,
                g: (((CHANGE_PER_LINE_X * y as f64) as u32) % 0xFF) as u8,
                b: u8::min(
                    (((CHANGE_PER_LINE_Y * x as f64) as u32) & 0xFF) as u8,
                    counter as u8,
                ),
            };

            if x == (counter) as usize {
                image[fake_x * BUFFER_Y + y].r ^= 0xFF;
                image[fake_x * BUFFER_Y + y].g ^= 0xFF;
                image[fake_x * BUFFER_Y + y].b ^= 0xFF;
            }
        }
    }
}

fn send_image_mmsg(
    image: &[Pixel],
    src_mac: MacAddr,
    dst_mac: MacAddr,
    socket_address: &mut sockaddr_ll,
    sockfd: i32,
    start: Instant,
) {
    unsafe {
        let after_image = start.elapsed();

        let chunks = image.chunks(CHUNK_SIZE);

        let mut iovecs = [mem::zeroed::<iovec>(); CHUNK_COUNT];
        let mut msgs = [mem::zeroed::<mmsghdr>(); CHUNK_COUNT];

        let mut ethernet_packets = [[0u8; MutableEthernetPacket::minimum_packet_size()
            + PAYLOAD_SIZE_SENDER
            + HEADER_SIZE]; CHUNK_COUNT];
        for (package_id, chunk) in chunks.enumerate() {
            // Convert the pixel data to bytes
            let mut payload = vec![0_u8; PAYLOAD_SIZE_SENDER];
            for (index, pixel) in chunk.iter().enumerate() {
                let pbytes = pixel_to_bytes(ColorFormat::BRG, pixel);
                payload[index * BYTES_PER_PIXEL..(index + 1) * BYTES_PER_PIXEL]
                    .copy_from_slice(&pbytes);
            }

            // Create the whole payload
            let packet: LinsnSenderPacket = LinsnSenderPacket {
                header: match package_id {
                    0 => LinsnHeader::chunk_start(src_mac),
                    _ => LinsnHeader::empty(package_id as u32),
                },
                payload: payload.try_into().unwrap(),
            };
            ethernet_packets[package_id]
                .copy_from_slice(packet.as_ethernet(Some(src_mac), Some(dst_mac)).packet());

            // iovec points to the packet buffer
            iovecs[package_id].iov_base = ethernet_packets[package_id].as_ptr() as *mut c_void;
            iovecs[package_id].iov_len = ethernet_packets[package_id].len();

            // mmsghdr contains message headers
            msgs[package_id].msg_hdr.msg_iov = &mut iovecs[package_id];
            msgs[package_id].msg_hdr.msg_iovlen = 1;
            msgs[package_id].msg_hdr.msg_name = socket_address as *mut sockaddr_ll as *mut c_void;
            msgs[package_id].msg_hdr.msg_namelen = mem::size_of::<sockaddr_ll>() as u32;
            msgs[package_id].msg_hdr.msg_control = ptr::null_mut();
            msgs[package_id].msg_hdr.msg_controllen = 0;
            msgs[package_id].msg_hdr.msg_flags = 0;
            msgs[package_id].msg_len = 0;
        }
        let sending_start = Instant::now();
        let ret = sendmmsg(sockfd, msgs.as_mut_ptr(), ethernet_packets.len() as u32, 0);
        if ret == -1 {
            eprintln!("Failed to send packets");
            close(sockfd);
            return;
        }

        let sending_done: Duration = sending_start.elapsed();
        thread::sleep(Duration::from_millis(5 - sending_done.as_millis() as u64));
        let sleeping_done: Duration = sending_start.elapsed();
        let total = start.elapsed();

        // else {
        //     println!("Successfully sent {} packets", ret);
        // }
        if total
            .abs_diff(sleeping_done.abs_diff(sending_done))
            .as_millis()
            > 1
        {
            println!(
                "Total: {:.0?} Image: {:.0?} Sending: {:.0?} Sleeping: {:.0?}",
                start.elapsed(),
                after_image,
                sending_done,
                sleeping_done.abs_diff(sending_done),
            );
        }
    }
}

fn send_image(
    image: &[Pixel],
    src_mac: MacAddr,
    dst_mac: MacAddr,
    tx: &mut Box<dyn datalink::DataLinkSender>,
) {
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
                0 => LinsnHeader::chunk_start(src_mac),
                _ => LinsnHeader::empty(package_id as u32),
            },
            payload: payload.try_into().unwrap(),
        };
        let ethernet_packet = packet.as_ethernet(Some(src_mac), Some(dst_mac));
        match tx.send_to(ethernet_packet.packet(), None) {
            Some(Ok(_)) => (),
            Some(Err(e)) => eprintln!("Failed to send packet: {}", e),
            None => eprintln!("Failed to send packet: No response"),
        }
    }
}
fn main() {
    init_screencapture();

    unsafe {
        let args: Vec<String> = std::env::args().collect();
        if args.len() < 2 {
            eprintln!("Usage: {} <interface_name>", args[0]);
            return;
        }
        let interface_name = args[1].as_str();

        let interfaces = datalink::interfaces();
        let interface = interfaces
            .into_iter()
            .find(|iface| iface.name == interface_name)
            .expect("Network interface not found");

        let channel: datalink::Channel = datalink::channel(&interface, Default::default()).unwrap();
        let mut tx = match channel {
            Channel::Ethernet(tx, _) => tx,
            _ => panic!("Unhandled channel type"),
        };

        let if_name = CString::new(interface_name).unwrap();
        let if_index = if_nametoindex(if_name.as_ptr());
        if if_index == 0 {
            eprintln!("Failed to get interface index");
            return;
        }

        let sockfd = socket(AF_PACKET, SOCK_RAW, (ETH_P_ALL as u16).to_be() as i32);
        if sockfd == -1 {
            eprintln!("Failed to create socket");
            return;
        }
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
            eprintln!("failed to get socket params");
            return;
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
            eprintln!("failed to get socket params");
            return;
        }

        println!("Send Buffer size: {:}", n);

        let src_mac = interface.mac.unwrap_or(MacAddr::broadcast());
        let dst_mac = MacAddr::zero();

        let dest_mac_byte = [
            dst_mac.0, dst_mac.1, dst_mac.2, dst_mac.3, dst_mac.4, dst_mac.5,
        ];

        let mut socket_address = sockaddr_ll {
            sll_family: AF_PACKET as u16,
            sll_protocol: (ETH_P_ALL as u16).to_be(),
            sll_ifindex: if_index as i32,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: ETH_ALEN as u8,
            sll_addr: [0; 8],
        };
        ptr::copy_nonoverlapping(
            dest_mac_byte.as_ptr(),
            socket_address.sll_addr.as_mut_ptr(),
            ETH_ALEN as usize,
        );

        let mut image = vec![Pixel { r: 0, g: 0, b: 0 }; BUFFER_Y * BUFFER_X];
        let mut counter = 0xFFi16;
        let mut image_counter = 0u128;
        let mut direction = 1i16;
        loop {
            // let start: Instant = Instant::now();
            fill_image(&mut image, counter, image_counter);
            if counter >= 0xFF {
                direction = -1;
            } else if counter < 0x01 {
                direction = 1;
            }
            counter += direction;
            image_counter += 1;
            //send_image_mmsg(&image, src_mac, dst_mac, &mut socket_address, sockfd, start)
            send_image(&image, src_mac, dst_mac, &mut tx)
        }
    }
}
