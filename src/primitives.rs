use std::sync::{mpsc::Sender, Arc};

use image::{imageops::{flip_horizontal, flip_vertical, resize}, DynamicImage, ImageBuffer, Rgb, Rgba};
use pnet::util::MacAddr;

use crate::{linsn::{LINSN_FRAME_HEIGHT, LINSN_FRAME_WIDTH}, socket::LinsnSocket, sprite::AnimatedSprite};

pub struct Panel {
    pub width: usize, 
    pub height: usize,
    image_buffer_active: Vec<Rgb<u8>>,
    image_buffer_inactive: Vec<Rgb<u8>>,
    pub double_buffering: bool,
    sprites: Vec<AnimatedSprite>,
    flip: bool,
}

impl Panel {
pub fn new(width : usize, height: usize, double_buffering: bool, flip: bool) -> Self {
    let image_buffer_active = vec![
        Rgb([0x69,0x20,0x69]);
        LINSN_FRAME_WIDTH as usize
            * LINSN_FRAME_HEIGHT as usize
    ];
    let image_buffer_inactive = vec![
        Rgb([0x00,0x00,0x00]);
        LINSN_FRAME_WIDTH as usize
            * LINSN_FRAME_HEIGHT as usize
    ];
    Panel {
        width,
        height,
        image_buffer_active,
        image_buffer_inactive,
        double_buffering,
        sprites: vec![],
        flip
    }
}

pub fn clear(&mut self) {
    for i in 0..self.width {
        for y in 0..self.height {
            self.set_pixel(i as i32, y as i32, Rgba([0u8,0u8,0u8,0xFFu8]));
        }
    }
}

pub fn set_pixel(&mut self,dest_x: i32, dest_y: i32, pixel: Rgba<u8>) {
    if dest_x < 0 || dest_y < 0 || dest_x >= self.width as i32 || dest_y > self.height as i32 {
        return;
    }
    let target_width = LINSN_FRAME_WIDTH as usize;
    let stride = (dest_y + 1) as usize * target_width ;

    let buffer = match self.double_buffering {
        true => &mut self.image_buffer_inactive,
        false => &mut self.image_buffer_active,
    };
    
    let alpha = pixel[3];

    if alpha == 0 {
        return
    }

    if alpha != 0xFF {
        let factor = alpha as f32 / 0xFF as f32;
        let old = buffer[stride + dest_x as usize];
        let r = ((old[0] as f32 * (1.0-factor)) + (pixel[0] as f32 * factor)) as u8;
        let g = ((old[1] as f32 * (1.0-factor)) + (pixel[1] as f32 * factor)) as u8;
        let b = ((old[2] as f32 * (1.0-factor)) + (pixel[2] as f32 * factor)) as u8;
        buffer[stride + dest_x as usize]= Rgb([r,g,b]);
    } else {
        buffer[stride + dest_x as usize] = Rgb([pixel[0], pixel[1], pixel[2]]);
    }
}

pub fn draw_image(&mut self, dest_x: i32, dest_y: i32, image: &DynamicImage, scale: f32, flip_x : bool, flip_y: bool) {

    let nwidth = (image.width() as f32 *scale) as u32;
    let nheight = (image.height()as f32 *scale) as u32;
    if nwidth <= 0 || nheight <= 0 {
        return;
    }
    let mut image = resize(
        image, 
        nwidth,
        nheight,
        image::imageops::FilterType::Nearest);

    if flip_x {
        image = flip_horizontal(&image);
    }

    if flip_y && ! self.flip || self.flip && !flip_y {
        image = flip_vertical(&image);
    }

    for row in 0..image.height() {
        for i in 0..image.width() {
            self.set_pixel(dest_x + i as i32, dest_y+row as i32, image[(i as u32, row as u32)]);
        }
    }
}
pub fn send(&mut self, sender: Arc<dyn LinsnSocket>, dst_mac: MacAddr) {
    if self.double_buffering {
        panic!("not implemented yet");
    }

    sender.send(&self.image_buffer_active, dst_mac);
}
}