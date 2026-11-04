//! Saves frames of the sample deck as PNGs: `snapshot <out-dir> <secs>...`

use kn_export::Offscreen;
use kn_media::MediaLibrary;
use kn_model::Project;
use kn_render::RenderContext;
use kn_time::Tick;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::PathBuf::from(&args[0]);
    let (w, h) = (960, 540);
    let mut gpu = Offscreen::new(w, h).expect("a GPU");
    let mut cx = RenderContext::new();
    let mut rgba = Vec::new();
    for secs in &args[1..] {
        let t = Tick::from_millis((secs.parse::<f64>().expect("seconds") * 1000.0) as i64);
        gpu.render(&Project::sample(), &MediaLibrary::new(), t, &mut cx, &mut rgba).expect("render");
        let img = image::RgbaImage::from_raw(w, h, rgba.clone()).expect("size");
        img.save(dir.join(format!("frame-{secs}.png"))).expect("save");
    }
}
