use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use gstreamer::BufferRef;
use image::imageops::resize;
use image::DynamicImage;
use image::ImageBuffer;
use image::Rgb;
use libc::rand;
use libc::size_t;
use linsn::LINSN_FRAME_HEIGHT;
use linsn::LINSN_FRAME_WIDTH;
use pnet::util::MacAddr;
use primitives::Panel;
use rand::Rng;
use screen_capture::init_gstreamer;
use socket::BatchedSocketSender;
use socket::LinsnSocket;
use socket::SimpleSocketSender;
use sprite::load_image_directory;
use sprite::AnimatedSprite;
use std::thread;

mod linsn;
mod primitives;
mod screen_capture;
mod socket;
mod sprite;

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

    let use_batched_sending = true;

    let sender: Arc<dyn LinsnSocket + Send> = if use_batched_sending {
        Arc::new(BatchedSocketSender::new(interface_name))
    } else {
        Arc::new(SimpleSocketSender::new(interface_name))
    };

    // init_gstreamer(play_demo_file, PANEL_X, PANEL_Y, {
    //     let linsn_image_lock = Arc::clone(&inactive_buffer);
    //     let should_flip = should_flip.clone();
    //     move |buffer: &BufferRef, width: u32, height: u32, bytes_per_pixel: u32| {
    //         // Map the buffer to read frame data
    //         let map = buffer
    //             .map_readable()
    //             .expect("Failed to map buffer readable");

    //         // Webcams often use Yuv2. We therefore enforve BGRx in gstreamer for now
    //         let row_stride = (width * bytes_per_pixel) as usize;
    //         let (copy_height, copy_width) =(height.min(PANEL_X as u32), width.min(PANEL_Y as u32));

    //         let mut linsn_image = linsn_image_lock.lock().expect("Mutex Poisend");
    //         for y in 0..copy_height as usize {
    //             for x in 0..copy_width as usize {
    //                 let offset = y * row_stride + x * bytes_per_pixel as usize;
    //                 let b = map[offset] as u8;
    //                 let g = map[offset + 1] as u8;
    //                 let r = map[offset + 2] as u8;
    //                 linsn_image[((y + 1) * LINSN_FRAME_WIDTH as usize) + x] = Pixel::new(r, g, b);
    //             }
    //         }

    //         should_flip.store(true, std::sync::atomic::Ordering::Relaxed);
    //     }
    // });

    // loop {
    //     let mut active = active_buffer.lock().unwrap();

    //     if should_flip.load(std::sync::atomic::Ordering::Relaxed) {
    //         let mut inactive = inactive_buffer.lock().unwrap();
    //         std::mem::swap(&mut *active, &mut *inactive);
    //         should_flip.store(false, std::sync::atomic::Ordering::Relaxed);
    //     }
    //     sender.send(&active, dst_mac);
    //     thread::sleep(Duration::from_millis(0));
    // }

    // let mut panel = Panel::new(192, 192, false, false);

    // let dragon_imgs = load_image_directory("./dragon");
    // let mut dragon1 = AnimatedSprite::new(dragon_imgs.clone(), 2.5, sprite::LoopMode::PingPong, 2.0);
    // let mut dragon2 = AnimatedSprite::new(dragon_imgs.clone(), 2.5, sprite::LoopMode::PingPong, 2.0);
    // let mut dragon3 = AnimatedSprite::new(dragon_imgs.clone(), 2.5, sprite::LoopMode::PingPong, 2.0);
    // let mut dragon4 = AnimatedSprite::new(dragon_imgs, 2.5, sprite::LoopMode::PingPong, 2.0);

    // let mut time: f32 = 0.0;
    // let mut rng = rand::thread_rng();
    // let d1_x = rng.gen_range(0..panel.width) as i32;
    // let d1_y = rng.gen_range(0..panel.height) as i32;
    // let d2_x = rng.gen_range(0..panel.width)as i32;
    // let d2_y = rng.gen_range(0..panel.height)as i32;
    // let d3_x = rng.gen_range(0..panel.width)as i32;
    // let d3_y = rng.gen_range(0..panel.height)as i32;
    // let d4_x = rng.gen_range(0..panel.width)as i32;
    // let d4_y = rng.gen_range(0..panel.height)as i32;

    // loop {
    //         time += 0.02;
    //         panel.clear();

    //         dragon1.draw(&mut panel, d1_x, d1_y);
    //         dragon2.draw(&mut panel, d2_x + (time.sin() * 64.0) as i32, d2_y);
    //         dragon3.draw(&mut panel, d3_x, d3_y + ((time).cos() * 32.0) as i32);
    //         dragon4.set_scale(3.0+ ((time).cos() * 3.0));
    //         dragon4.draw(&mut panel, d4_x, d4_y);

    //         panel.send(sender.clone(), dst_mac);
    //         thread::sleep(Duration::from_millis(0));
    //     }

    let mut panel = Panel::new(192, 192, false, false);
    
    // Train
    let train_imgs = load_image_directory("./assets/train");
    let mut train = AnimatedSprite::new(train_imgs.clone(), 1.0, sprite::LoopMode::PingPong, 2.0);

    // Tracks
    let tracks: DynamicImage = image::open("assets/train_tracks.png").unwrap();

    // Background
    let grass: DynamicImage = image::open("assets/grass.png").unwrap();
    let backgrounds = load_image_directory("assets/background");

    // Sky 
    let skys = load_image_directory("assets/sky");

    let before = Instant::now();
    loop {
        let ts = Instant::now()-before;
        panel.clear();

        for i in 0..6 {
            panel.draw_image((i * 32) , 192-32, &grass, 2.0, false, false);
        }

        for i in 0..4 {
           train.draw(&mut panel, 64 * i, 192-32);
        }

        let track_offset = ((ts.as_millis() / 200) % 32 )as i32;
        for i in 0..8 {
            panel.draw_image((i * 32) - track_offset, 192-32, &tracks, 2.0, false, false);
        }
    
        let background_offset = ((ts.as_millis() / 500) % 192 )as i32;
        for i in 0usize..16 {
            panel.draw_image((i as i32 * 32) - background_offset, 192-64, &backgrounds[i % backgrounds.len()], 2.0, false, false);
        }

        let sky_offset = ((ts.as_millis() / 500) % 192 )as i32;
        for i in 0usize..16 {
            panel.draw_image((i as i32 * 32) - sky_offset, 96, &skys[i % skys.len()], 2.0, false, false);
            panel.draw_image((i as i32 * 32) - sky_offset, 64, &skys[(i + 6) % skys.len()], 2.0, false, false);
            panel.draw_image((i as i32 * 32) - sky_offset, 32, &skys[(i + 8) % skys.len()], 2.0, false, false);
            panel.draw_image((i as i32 * 32) - sky_offset, 0, &skys[(i + 11) % skys.len()], 2.0, false, false);
        }

        panel.send(sender.clone(), dst_mac);
    }
}
