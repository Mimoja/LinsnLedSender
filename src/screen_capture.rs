use std::f64::consts::PI;
use std::sync::Arc;

use gstreamer::glib;
use gstreamer::glib::EnumClass;
use gstreamer::glib::EnumValue;
use gstreamer::prelude::*;
use gstreamer::BufferRef;
use gstreamer::Caps;
use gstreamer::Fraction;
use gstreamer::{ElementFactory, Pipeline};
use gstreamer_app::AppSink;
use gstreamer_video::VideoInfo;

// Helper function for most webcam streams
fn yuv_to_rgb(y: f32, u: f32, v: f32) -> [u8; 3] {
    let r = (y + 1.402 * v).clamp(0.0, 255.0) as u8;
    let g = (y - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8;
    let b = (y + 1.772 * u).clamp(0.0, 255.0) as u8;
    [r, g, b]
}

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

pub fn init_gstreamer<F>(play_file: bool, panelx: usize, panely: usize, on_frame: F)
where
    F: Fn(&BufferRef, u32, u32, u32) + Send + Sync + 'static,
{
    gstreamer::init().expect("Failed to initialize GStreamer");

    // Create a GStreamer pipeline
    let pipeline = Pipeline::with_name("screen-capture");

    // Set up the source element
    let src = if play_file {
        // filesrc with decodebin for video files
        let filesrc = ElementFactory::make("filesrc")
            .property("location", "big_buck_bunny_1080p_h264.mov") // Set the path to your file
            .build()
            .unwrap();
        let decode = ElementFactory::make("decodebin").build().unwrap();

        pipeline.add_many(&[&filesrc, &decode]).unwrap();
        filesrc.link(&decode).unwrap();

        // Return decodebin as the source
        decode
    } else {
        // ximagesrc for screen capture
        let ximagesrc = ElementFactory::make("ximagesrc")
            .property("use-damage", false)
            .property("show-pointer", true)
            .build()
            .unwrap();

        pipeline.add(&ximagesrc).unwrap();
        ximagesrc
    };

    let convert = ElementFactory::make("videoconvert").build().unwrap();
    let scale = ElementFactory::make("videoconvertscale")
        .property("add-borders", false)
        .build()
        .unwrap();

    let rotate = ElementFactory::make("rotate")
        .property("angle", PI)
        .property_from_str("off-edge-pixels", "clamp")
        .build()
        .unwrap();

    let rate = ElementFactory::make("videorate").build().unwrap();

    let clocksync = ElementFactory::make("clocksync").build().unwrap();

    let tee = ElementFactory::make("tee").build().unwrap();
    let queue_local_window = ElementFactory::make("queue").build().unwrap();
    let queue_leds = ElementFactory::make("queue").build().unwrap();

    let mut caps = Caps::builder("video/x-raw").field("format", "BGRx");

    if play_file {
        caps = caps
            .field("height", panelx as i32)
            .field("width", panely as i32)
    } else {
        caps = caps
            .field("height", panelx as i32)
            .field("width", panely as i32)
            .field("framerate", &Fraction::new(120, 1));
    }
    let caps = caps.build();

    // Configure appsink properties to enable frame capture
    let sink_leds = ElementFactory::make("appsink")
        .property("emit-signals", true)
        .property("sync", false)
        .property("caps", &caps)
        .build()
        .unwrap();

    let scale_window = ElementFactory::make("videoconvertscale")
        .property("add-borders", true)
        .build()
        .unwrap();
    let sink_window = ElementFactory::make("ximagesink")
        .property("force-aspect-ratio", true)
        .build()
        .unwrap();

    // Add elements to the pipeline
    pipeline
        .add_many(&[
            &convert,
            &scale,
            &rotate,
            &rate,
            &clocksync,
            &tee,
            &queue_leds,
            &queue_local_window,
            &sink_leds,
            &scale_window,
        ])
        .expect("Failed to add elements to the pipeline");

    if false {
        pipeline
            .add_many(&[&sink_window])
            .expect("Failed to add elements to the pipeline");
    }

    // Link elements in the pipeline
    gstreamer::Element::link_many(&[&convert, &rotate, &scale, &rate, &clocksync, &tee])
        .expect("Failed to link elements");

    // Dynamically link decodebin to videoconvert if filesrc is used
    if play_file {
        src.connect_pad_added(move |_, src_pad| {
            let sink_pad = convert.static_pad("sink").expect("Failed to get sink pad");
            if !sink_pad.is_linked() {
                src_pad
                    .link(&sink_pad)
                    .expect("Failed to link decodebin to videoconvert");
            }
        });
    } else {
        // Directly link ximagesrc to videoconvert if screen capture is used
        src.link(&convert).unwrap();
    }

    //tee.link(&queue_local_window).unwrap();
    tee.link(&queue_leds).unwrap();

    // Link the rest of the elements
    //queue_local_window.link(&scale_window).unwrap();
    queue_leds.link(&sink_leds).unwrap();

    //scale_window.link(&sink_window).unwrap();

    // Wrap the callback in an Arc<Mutex> for safe sharing across threads
    let on_frame = Arc::new(on_frame);
    let on_frame_clone = Arc::clone(&on_frame);

    let appsink = sink_leds
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
                // Call the user-provided callback with the 2D array and resolution
                on_frame_clone(buffer, width, height, 4);
                Ok(gstreamer::FlowSuccess::Ok)
            })
            .build(),
    );

    // Start the pipeline
    dbg!(pipeline.set_state(gstreamer::State::Playing))
        .expect("Unable to set pipeline to `Playing` state");
}
