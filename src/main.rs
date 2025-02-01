use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use gstreamer::BufferRef;
use linsn::Pixel;
use linsn::LINSN_FRAME_HEIGHT;
use linsn::LINSN_FRAME_WIDTH;
use pnet::util::MacAddr;
use screen_capture::init_gstreamer;
use socket::BatchedSocketSender;
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

    let copy_all_pixel = false;
    let send_all_pixel = false;
    let use_batched_sending = true;
    let play_demo_file = true;

    let sender: Arc<Mutex<dyn LinsnSocket + Send>> = if use_batched_sending {
        Arc::new(Mutex::new(BatchedSocketSender::new(interface_name)))
    } else {
        Arc::new(Mutex::new(SimpleSocketSender::new(interface_name)))
    };

    // This is the Linsn Frame to send in the end
    let send_frame_height = if send_all_pixel {
        LINSN_FRAME_HEIGHT
    } else {
        PANEL_X as u32 + 1
    };

    let active_buffer = Arc::new(Mutex::new(vec![
        Pixel::white();
        LINSN_FRAME_WIDTH as usize
            * LINSN_FRAME_HEIGHT as usize
    ]));
    let inactive_buffer = Arc::new(Mutex::new(vec![
        Pixel::white();
        LINSN_FRAME_WIDTH as usize
            * LINSN_FRAME_HEIGHT as usize
    ]));

    let should_flip = Arc::new(AtomicBool::new(false));

    init_gstreamer(play_demo_file, PANEL_X, PANEL_Y, {
        let linsn_image_lock = Arc::clone(&inactive_buffer);
        let should_flip = should_flip.clone();
        move |buffer: &BufferRef, width: u32, height: u32, bytes_per_pixel: u32| {
            // Map the buffer to read frame data
            let map = buffer
                .map_readable()
                .expect("Failed to map buffer readable");

            // Webcams often use Yuv2. We therefore enforve BGRx in gstreamer for now
            let row_stride = (width * bytes_per_pixel) as usize;

            // descide if we want to only fill what we are actually going to display
            let (copy_height, copy_width) = if copy_all_pixel {
                (send_frame_height as u32, LINSN_FRAME_WIDTH as u32)
            } else {
                (height.min(PANEL_X as u32), width.min(PANEL_Y as u32))
            };

            let mut linsn_image = linsn_image_lock.lock().expect("Mutex Poisend");
            for y in 0..copy_height as usize {
                for x in 0..copy_width as usize {
                    let offset = y * row_stride + x * bytes_per_pixel as usize;
                    let b = map[offset] as u8;
                    let g = map[offset + 1] as u8;
                    let r = map[offset + 2] as u8;
                    linsn_image[((y + 1) * LINSN_FRAME_WIDTH as usize) + x] = Pixel::new(r, g, b);
                }
            }

            should_flip.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });

    loop {
        let mut active = active_buffer.lock().unwrap();

        if should_flip.load(std::sync::atomic::Ordering::Relaxed) {
            let mut inactive = inactive_buffer.lock().unwrap();
            std::mem::swap(&mut *active, &mut *inactive);
            should_flip.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        // Lock the sender and send the image
        let sender = sender.lock().expect("Failed to lock sender");
        sender.send(&active, dst_mac);
        thread::sleep(Duration::from_millis(0));
    }
}
