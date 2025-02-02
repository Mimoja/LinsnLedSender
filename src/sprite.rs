use std::{
    fs,
    time::{Duration, Instant},
};

use image::DynamicImage;
use rand::{rngs::{self, ThreadRng}, Rng};

use crate::primitives::Panel;

pub fn load_image_directory(dir: &str) -> Vec<DynamicImage> {
    let mut images_data = Vec::new();

    // Iterate over all entries in the folder.
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        // Check the file extension (case-insensitively) to see if it’s a PNG.
        if path
            .extension()
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

#[derive(Clone)]

pub struct AnimatedPath {
    points: Vec<(i32, i32, u64, bool)>,
    start: Option<Instant>,
    finished: bool,
}

impl AnimatedPath {
    pub fn new(points: Vec<(i32, i32, u64, bool)>) -> Self {
        AnimatedPath{
            points,
            start: None,
            finished: false,
        }
    }

    pub fn new_random(rng: &mut ThreadRng, duration: u64) -> AnimatedPath{
        let left = rng.gen_bool(0.5);
        let mut start = -32;
        let mut end = 200;
        let height = rng.gen_range(0..64);
        if left {
            std::mem::swap(&mut start, &mut end);
        }
        let dragon_animation: AnimatedPath = AnimatedPath::new(vec![(start, height, 0, left), (end, height,duration, left)]);
        dragon_animation
    }

    pub fn reset(&mut self) {
        self.start = None;
        self.finished = false;
    }
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
    path: Option<AnimatedPath>,
    position: (i32, i32),
}

impl AnimatedSprite {
    pub fn new(images: Vec<DynamicImage>, framerate: f32, loop_mode: LoopMode, scale: f32) -> Self {
        AnimatedSprite {
            images,
            position: (0, 0),
            frame_duration: Duration::from_millis((1000.0 / framerate) as u64),
            last_update: Instant::now(),
            current_frame: 0,
            loop_mode,
            _direction: 1,
            scale,
            flip: false,
            path: None,
        }
    }

    pub fn set_scale(&mut self, scale: f32) {
        if scale <= 0.0 {
            return;
        }
        self.scale = scale
    }

    pub fn flip(&mut self) {
        self.flip = !self.flip;
    }

    pub fn set_animation(&mut self, path: AnimatedPath){
        self.path = Some(path.clone())
    }

    pub fn set_position(&mut self, x: i32, y: i32) {
        self.position = (x, y)
    }

    pub fn draw_at(&mut self, panel: &mut Panel, x: i32, y: i32) {
        self.set_position(x, y);
        self.draw(panel);
    }

    pub fn reset_animation(&mut self) {
        println!("Restarting animation");
        self.path.as_mut().unwrap().reset();
    }

    pub fn has_finished(&mut self)  -> bool{
        self.path.as_ref().unwrap().finished
    }

    pub fn animate(&mut self, panel: &mut Panel) {
        if self.path.is_none() {
            return;
        }
        if (self.has_finished()) {
            self.draw(panel);
            return;
        }
        let mut new_position: Option<(i32, i32, bool)> = None;
        {
            let path = &mut self.path.as_mut().unwrap();
            if path.start.is_none() {
                println!("Starting animation");
                path.start = Some(Instant::now())
            }

            let mut time = Instant::now()- path.start.unwrap() ;
            path.finished = true;
            for i in 0..path.points.len() - 1 {
                let current_point = path.points[i];
                let next_point = path.points[i+1];
                let target_duration = Duration::from_millis( next_point.2);
                if time>target_duration {
                    time -= target_duration;
                    continue;
                }
                path.finished = false;
                // println!("Animating from point {} to {}: {}/{} -> {}/{}", i , i+1, current_point.0, current_point.1, next_point.0, next_point.1);

                let factor = time.as_millis() as f64/ next_point.2 as f64;

                // println!("Time: {}, Factor: {} Start: {:?}", time.as_millis(), factor, path.start.unwrap());

                let new_x = (current_point.0 as f64 * (1.0 -factor) + next_point.0 as f64* (factor));
                let new_y = (current_point.1 as f64 * (1.0 - factor) + next_point.1 as f64* (factor));
                let flipped = next_point.3;
                new_position = Some((new_x as i32, new_y as i32, flipped));
                break;
            }
        }
        if let Some(pos) = new_position{
            self.flip = pos.2;
            self.draw_at(panel, pos.0, pos.1);
        }
    }

    pub fn draw(&mut self, panel: &mut Panel) {
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
        panel.draw_image(
            self.position.0,
            self.position.1,
            &self.images[self.current_frame],
            self.scale,
            self.flip,
            false,
        );
    }
}
