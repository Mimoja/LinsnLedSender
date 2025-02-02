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
use libc::size_t;
use linsn::LINSN_FRAME_HEIGHT;
use linsn::LINSN_FRAME_WIDTH;
use pnet::util::MacAddr;
use primitives::Panel;
use rand::prelude::*;
use screen_capture::init_gstreamer;
use socket::BatchedSocketSender;
use socket::LinsnSocket;
use socket::SimpleSocketSender;
use sprite::load_image_directory;
use sprite::AnimatedPath;
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

    let mut rng = rand::rngs::ThreadRng::default();
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
    let sky1 = load_image_directory("assets/sky");
    let sky2 = load_image_directory("assets/sky/level2");
    let sky3 = load_image_directory("assets/sky/level3");
    let sky4 = load_image_directory("assets/sky/level4");

    // Dragon
    let dragon_imgs = load_image_directory("assets/dragon");
    let mut dragon = AnimatedSprite::new(dragon_imgs, 2.5, sprite::LoopMode::PingPong, 2.0);
    dragon.set_animation(  AnimatedPath::new_random(&mut rng, 16000));

    // Schwebebahn
    let schwebebahn = load_image_directory("assets/schwebebahn");

    let mut skys_data = vec![sky4, sky3, sky2, sky1];
    let mut skys = vec![vec![], vec![], vec![], vec![]];
    let before = Instant::now();

    loop {
        let ts = Instant::now() - before;
        panel.clear();

        for i in 0..6 {
            panel.draw_image((i * 32), 192 - 32, &grass, 2.0, false, false);
        }

        for i in 0..4 {
            train.draw_at(&mut panel, 64 * i, 192 - 32);
        }

        let track_offset = ((ts.as_millis() / 200) % 32) as i32;
        for i in 0..8 {
            panel.draw_image(
                (i * 32) - track_offset,
                192 - 32,
                &tracks,
                2.0,
                false,
                false,
            );
        }

        let background_offset = ((ts.as_millis() / 500) % 192) as i32;
        for i in 0usize..16 {
            panel.draw_image(
                (i as i32 * 32) - background_offset,
                192 - 64,
                &backgrounds[i % backgrounds.len()],
                2.0,
                false,
                false,
            );
        }

        let sky_offset = ((ts.as_millis() / 500) % 32) as i32;
        for (i, (sky, sky_data)) in skys.iter_mut().zip(&skys_data).enumerate() {
            while sky.len() < 8 {
                sky.push(&sky_data[rng.next_u64() as usize % sky_data.len()]);
            }

            let mut should_pop = false;
            for (j, s) in sky.clone().into_iter().enumerate() {
                let x = (j as i32 * 32) - sky_offset;
                panel.draw_image(x, i as i32 * 32, s, 2.0, false, false);

                if i == 0 {
                    if x % 32 == 0 {
                        should_pop = true;
                    }
                }
            }

            if should_pop {
                sky.pop();
            }
        }

        if dragon.has_finished() && rng.gen_bool(0.002) {
            dragon.set_animation(  AnimatedPath::new_random(&mut rng, 16000));
        }
        dragon.animate(&mut panel);

        /*
        let schwebebahn_offset = ((ts.as_millis() / 50) % 800) as i32;
        panel.draw_image((schwebebahn_offset - 256) * -1, 138, &schwebebahn[1], 2.0, false, false);
        panel.draw_image(0, 132, &schwebebahn[0], 2.0, false, false);
        */

        /*
        let schwebebahn_offset = ((ts.as_millis() / 50) % 800) as i32;
        panel.draw_image((schwebebahn_offset - 256) * -1, 138, &schwebebahn[1], 2.0, false, false);
        panel.draw_image(0, 132, &schwebebahn[0], 2.0, false, false);
        */

        panel.send(sender.clone(), dst_mac);
    }
}
