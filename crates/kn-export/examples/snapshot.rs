//! Saves a contact sheet of every template's slides at rest:
//! `snapshot <out-dir>`

use kn_export::Offscreen;
use kn_layout::Template;

use kn_render::RenderContext;
use kn_time::Tick;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("an output folder"));
    let mut cx = RenderContext::new();
    let mut rgba = Vec::new();
    // Templates use the bundled sample artwork.
    let media = kn_media::sample_library();
    for template in Template::ALL {
        let project = kn_layout::template(template);
        let (cw, ch) = project.aspect.canvas_size();
        let w = 480u32;
        let h = ((f64::from(w) * ch / cw) as u32) & !1;
        let mut gpu = Offscreen::new(w, h).expect("a GPU");
        let mut sheet = image::RgbaImage::new(w * project.slides.len() as u32, h);
        let mut start = Tick::ZERO;
        for (i, slide) in project.slides.iter().enumerate() {
            let t = start + slide.duration - Tick::from_millis(10);
            gpu.render(&project, &media, t, &mut cx, &mut rgba).expect("render");
            let img = image::RgbaImage::from_raw(w, h, rgba.clone()).expect("size");
            image::imageops::overlay(&mut sheet, &img, i64::from(w) * i as i64, 0);
            start = start + slide.duration;
        }
        sheet.save(dir.join(format!("{}.png", template.name().replace(' ', "-")))).expect("save");
    }
}
