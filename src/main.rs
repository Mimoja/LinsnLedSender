use std::time::Duration;

use gstreamer::BufferRef;
use linsn::Pixel;
use pnet::util::MacAddr;
use screen_capture::init_screencapture;
use socket::LinsnSocket;
use socket::SimpleSocketSender;
use std::thread;

mod linsn;
mod screen_capture;
mod socket;

const PANEL_X: usize = 192;
const PANEL_Y: usize = 192;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <interface_name>", args[0]);
        return;
    }
    let interface_name = args[1].as_str();
    let dst_mac = MacAddr::zero();
    let sender = SimpleSocketSender::new(interface_name);

    init_screencapture(
        true,
        PANEL_X,
        PANEL_Y,
        move |buffer: &BufferRef, width: u32, height: u32, bytes_per_pixel: u32| {
            // Map the buffer to read frame data
            let map = buffer
                .map_readable()
                .expect("Failed to map buffer readable");

            // Webcams often use Yuv2. We therefore enforve BGRx in gstreamer for now
            let row_stride = (width * bytes_per_pixel) as usize;

            // This is the Lins Frame
            let mut linsn_image = vec![Pixel::white(); 1024 * (200)];

            // Only fill what we are actually going to display
            for y in 0..height.min(PANEL_X as u32) as usize {
                for x in 0..width.min(PANEL_Y as u32) as usize {
                    let offset = y * row_stride + x * 4;
                    let b = map[offset] as u8;
                    let g = map[offset + 1] as u8;
                    let r = map[offset + 2] as u8;
                    linsn_image[((y + 1) * 1024 as usize) + x] = Pixel::new(r, g, b);
                    linsn_image[((y + 1) * 1024 as usize) + x + 512] = Pixel::new(r, g, b);
                }
            }
            sender.clone().send(linsn_image, dst_mac);
        },
    );
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
