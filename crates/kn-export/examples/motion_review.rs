//! Render a visual review reel using the production preview/export renderer.
use kn_export::Offscreen;
use kn_media::MediaLibrary;
use kn_model::{Motion, Project, Transition};
use kn_render::RenderContext;
use kn_time::Tick;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(std::env::args().nth(1).expect("output directory"));
    fs::create_dir_all(&dir)?;
    let mut renderer = Offscreen::new(640, 360)?;
    let media = MediaLibrary::new();
    let mut cx = RenderContext::new();
    let mut rgba = Vec::new();
    for (label, transition, motion, exit) in Transition::ALL
        .into_iter()
        .map(|t| (format!("transition-{t:?}"), t, Motion::None, Motion::None))
        .chain(Motion::ALL.into_iter().map(|m| (format!("arrival-{m:?}"), Transition::Cut, m, Motion::None)))
        .chain(Motion::ALL.into_iter().map(|m| (format!("exit-{m:?}"), Transition::Cut, Motion::None, m)))
    {
        let mut project = Project::sample();
        for slide in &mut project.slides {
            slide.transition = transition;
            slide.entrance = kn_model::Entrance::Together;
            for item in &mut slide.elements {
                item.motion = motion;
                item.exit = exit;
            }
        }
        let mut file = fs::File::create(dir.join(format!("{label}.gif")))?;
        let mut gif = gif::Encoder::new(&mut file, 640, 360, &[])?;
        gif.set_repeat(gif::Repeat::Infinite)?;
        for frame in 0..45 {
            let ms = if label.starts_with("transition") {
                2800 + frame * 25
            } else if label.starts_with("exit") {
                2000 + frame * 22
            } else {
                frame * 25
            };
            renderer.render(&project, &media, Tick::from_millis(ms), &mut cx, &mut rgba)?;
            if [8, 14, 20, 26, 32].contains(&frame) {
                image::save_buffer(dir.join(format!("{label}-{frame}.png")), &rgba, 640, 360, image::ColorType::Rgba8)?;
            }
            let mut f = gif::Frame::from_rgba_speed(640, 360, &mut rgba, 10);
            f.delay = 3;
            gif.write_frame(&f)?;
        }
        println!("{label}");
    }
    Ok(())
}
