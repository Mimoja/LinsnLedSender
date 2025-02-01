use std::{fs, time::{Duration, Instant}};

use image::DynamicImage;

use crate::primitives::Panel;

pub fn load_image_directory(dir : &str) ->Vec<DynamicImage> {
    let mut images_data = Vec::new();

    // Iterate over all entries in the folder.
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        // Check the file extension (case-insensitively) to see if it’s a PNG.
        if path.extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.eq_ignore_ascii_case("png"))
            .unwrap_or(false)
        {
            // Open the image file.
            let img = image::open(&path).unwrap();

            images_data.push(img);
        }
    }
    println!("Loaded {} PNG images from {}", images_data.len(), dir);

    images_data
}

#[derive(Debug)]
pub enum LoopMode {
    Loop,     // Always loop from the beginning.
    PingPong, // Loop forward then backward.
}

pub struct AnimatedSprite {
    images: Vec<DynamicImage>,
    frame_duration: Duration,
    last_update: Instant,
    current_frame: usize,
    loop_mode: LoopMode,
    _direction: i32,
    scale: f32,
    flip: bool,
}

impl AnimatedSprite {
    pub fn new(images: Vec<DynamicImage>, framerate: f32, loop_mode: LoopMode, scale: f32) -> Self {
        AnimatedSprite {
            images,
            frame_duration: Duration::from_millis((1000.0 / framerate) as u64),
            last_update: Instant::now(),
            current_frame: 0,
            loop_mode,
            _direction: 1,
            scale,
            flip: false,
        }
    }

    pub fn set_scale(&mut self, scale: f32) {
        if scale <= 0.0 {
            return
        }
        self.scale = scale
    }

    pub fn flip(&mut self) {
        self.flip = !self.flip;
    }
    
    pub fn draw(&mut self, panel: &mut Panel, x: i32, y: i32) {
        let now = Instant::now();
        // Check if enough time has passed to advance the frame.
        if now.duration_since(self.last_update) >= self.frame_duration {
            self.last_update = now;
            match self.loop_mode {
                LoopMode::Loop => {
                    // Move to the next frame, wrapping back to 0.
                    self.current_frame = (self.current_frame + 1) % self.images.len();
                }
                LoopMode::PingPong => {
                    // Calculate the next frame index.
                    let next_frame = self.current_frame as i32 + self._direction;
                    if next_frame < 0 || next_frame >= self.images.len() as i32 {
                        // Reverse direction when hitting the edges.
                        self._direction = -self._direction;
                        self.current_frame = (self.current_frame as i32 + self._direction) as usize;
                    } else {
                        self.current_frame = next_frame as usize;
                    }
                }
            }
        }
        // Draw the current image using the provided drawing function.
        panel.draw_image( x, y , &self.images[self.current_frame], self.scale, self.flip, false);
    }
}