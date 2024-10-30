mod linsn;

use std::ops::DerefMut;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;

use gstreamer::prelude::*;
use gstreamer::Caps;
use gstreamer::Fraction;
use gstreamer::Structure;
use gstreamer::{ElementFactory, Pipeline};
use gstreamer_app::AppSink;
use gstreamer_app::AppSinkCallbacks;
use gstreamer_video::VideoInfo;
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
use pnet::datalink::DataLinkSender;
use pnet::packet::ethernet::MutableEthernetPacket;
use pnet::packet::Packet;
use pnet::util::MacAddr;
use std::ffi::CString;
use std::mem;
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread;

const PANEL_X: usize = 192;
const PANEL_Y: usize = 192;
const BYTES_PER_PIXEL: usize = 3;
const CHUNK_SIZE: usize = PAYLOAD_SIZE_SENDER / BYTES_PER_PIXEL;

fn yuv_to_rgb(y: f32, u: f32, v: f32) -> [u8; 3] {
    let r = (y + 1.402 * v).clamp(0.0, 255.0) as u8;
    let g = (y - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8;
    let b = (y + 1.772 * u).clamp(0.0, 255.0) as u8;
    [r, g, b]
}

fn init_screencapture<F>(on_frame: F)
where
    F: Fn(Vec<Pixel>, u32, u32) + Send + Sync + 'static,
{
    gstreamer::init().expect("Failed to initialize GStreamer");

    // Create a GStreamer pipeline
    let pipeline = Pipeline::with_name("screen-capture");
    let use_file = true;
    // // Create PipeWire source element (for Wayland screen capture)
    // let src = ElementFactory::make("pipewiresrc")
    //     .property("path", "/org/freedesktop/portal/desktop") // Common path for PipeWire screen capture
    //     .build()
    //     .unwrap();

    // Set up the source element based on the boolean flag
    let src = if use_file {
        // filesrc with decodebin for video files
        let filesrc = ElementFactory::make("filesrc")
            .property("location", "big_buck_bunny_1080p_h264.mov") // Set the path to your file
            .build()
            .unwrap();
        let decode = ElementFactory::make("decodebin").build().unwrap();

        pipeline.add_many(&[&filesrc, &decode]).unwrap();
        filesrc.link(&decode).unwrap();

        // Return decodebin as the source
        Some(decode)
    } else {
        // ximagesrc for screen capture
        let ximagesrc = ElementFactory::make("ximagesrc")
            .property("use-damage", false)
            .property("show-pointer", true)
            .build()
            .unwrap();

        pipeline.add(&ximagesrc).unwrap();
        Some(ximagesrc)
    };

    let convert = ElementFactory::make("videoconvert").build().unwrap();
    let scale = ElementFactory::make("videoconvertscale")
        .property("add-borders", true)
        .build()
        .unwrap();
    let rate = ElementFactory::make("videorate").build().unwrap();
    let clocksync = ElementFactory::make("clocksync").build().unwrap();
    let mut caps = Caps::builder("video/x-raw").field("format", "BGRx");

    if use_file {
        caps = caps
            .field("height", PANEL_X as i32)
            .field("width", PANEL_Y as i32)
    } else {
        caps = caps
            .field("height", PANEL_X as i32)
            .field("width", PANEL_Y as i32)
            .field("framerate", &Fraction::new(120, 1));
    }
    let caps = caps.build();

    // Configure appsink properties to enable frame capture
    let sink = ElementFactory::make("appsink")
        .property("emit-signals", true)
        .property("sync", false)
        .property("caps", &caps)
        .build()
        .unwrap();

    // Add elements to the pipeline
    pipeline
        .add_many(&[&convert, &scale, &rate, &clocksync, &sink])
        .expect("Failed to add elements to the pipeline");

    // Link elements in the pipeline
    gstreamer::Element::link_many(&[&convert, &scale, &rate, &clocksync, &sink])
        .expect("Failed to link elements");

    // Dynamically link decodebin to videoconvert if filesrc is used
    if use_file {
        if let Some(decode) = src {
            decode.connect_pad_added(move |_, src_pad| {
                let sink_pad = convert.static_pad("sink").expect("Failed to get sink pad");
                if !sink_pad.is_linked() {
                    src_pad
                        .link(&sink_pad)
                        .expect("Failed to link decodebin to videoconvert");
                }
            });
        }
    } else {
        // Directly link ximagesrc to videoconvert if screen capture is used
        if let Some(src_element) = src {
            src_element.link(&convert).unwrap();
        }
    }

    // Wrap the callback in an Arc<Mutex> for safe sharing across threads
    let on_frame = Arc::new(on_frame);
    let on_frame_clone = Arc::clone(&on_frame);

    let appsink = sink
        .dynamic_cast::<AppSink>()
        .expect("Sink element is expected to be an AppSink");

    appsink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                let sample = match appsink.pull_sample() {
                    Ok(sample) => sample,
                    Err(_) => return Err(gstreamer::FlowError::Eos),
                };

                // Get the buffer from the sample
                let buffer = sample.buffer().expect("Failed to get buffer from sample");
                // Get the caps and extract resolution
                let caps = sample.caps().expect("Failed to get caps from sample");
                // println!("Caps: {:?}", caps);

                let info = VideoInfo::from_caps(&caps).expect("Failed to get VideoInfo from caps");
                let width = info.width();
                let height = info.height();

                // Map the buffer to read frame data
                let map = buffer
                    .map_readable()
                    .expect("Failed to map buffer readable");

                // Assuming RGB format (3 bytes per pixel)
                let row_stride = (width * 4) as usize;
                let mut frame_2d = vec![Pixel::white(); 1024 * (200)];

                // For yuv2
                // for y in 0..height.min(1023) as usize {
                //     for x in (0..width.min(512) as usize).step_by(2) {
                //         // Get YUY2 data for two pixels
                //         let offset = y * row_stride + x * 2;
                //         let y0 = map[offset] as f32;
                //         let u = map[offset + 1] as f32 - 128.0;
                //         let y1 = map[offset + 2] as f32;
                //         let v = map[offset + 3] as f32 - 128.0;

                //         let p1 = yuv_to_rgb(y0, u, v);
                //         let p2 = yuv_to_rgb(y1, u, v);
                //         frame_2d[((y + 1) * 512 as usize) + x] = Pixel::new(p1[0], p1[1], p1[2]);
                //         frame_2d[((y + 1) * 512 as usize) + x + 1] =
                //             Pixel::new(p2[0], p2[1], p2[2]);
                //     }
                // }
                for y in 0..height.min(PANEL_Y as u32) as usize {
                    for x in (0..width.min(PANEL_X as u32) as usize) {
                        let offset = y * row_stride + x * 4;
                        let b = map[offset] as u8;
                        let g = map[offset + 1] as u8;
                        let r = map[offset + 2] as u8;
                        frame_2d[((y + 1) * 1024 as usize) + x] = Pixel::new(r, g, b);
                        frame_2d[((y + 1) * 1024 as usize) + x + 512] = Pixel::new(r, g, b);
                    }
                }
                // Call the user-provided callback with the 2D array and resolution
                on_frame_clone(frame_2d, width, height);
                Ok(gstreamer::FlowSuccess::Ok)
            })
            .build(),
    );

    // Start the pipeline
    pipeline
        .set_state(gstreamer::State::Playing)
        .expect("Unable to set pipeline to `Playing` state");
}

fn send_image(image: Vec<Pixel>, src_mac: MacAddr, dst_mac: MacAddr, tx: &mut dyn DataLinkSender) {
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
    let tx = match channel {
        Channel::Ethernet(tx, _) => tx,
        _ => panic!("Unhandled channel type"),
    };
    let src_mac = interface.mac.unwrap_or(MacAddr::broadcast());
    let dst_mac = MacAddr::zero();

    let tx = Arc::new(Mutex::new(tx));

    init_screencapture(move |image, width, height| {
        let tx = Arc::clone(&tx);

        // Lock the transmitter to send the image
        let mut tx = tx.lock().expect("Failed to acquire lock on transmitter");
        send_image(image, src_mac, dst_mac, tx.deref_mut().deref_mut());

        // println!("Captured frame of Resolution: {}x{}", width, height);
    });
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
