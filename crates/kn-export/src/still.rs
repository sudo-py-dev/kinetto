//! Other ways out: an animated GIF of the whole video, and a PDF with one
//! page per slide. Both use the same renderer as the preview.

use crate::gpu::{Gpu, Offscreen};
use crate::ExportError;
use kn_media::MediaLibrary;
use kn_model::Project;
use kn_render::RenderContext;
use kn_time::{FrameRate, Tick};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Width of exported GIFs; small enough to share in a chat.
pub const GIF_WIDTH: u32 = 640;
const GIF_FPS: FrameRate = FrameRate { num: 12, den: 1 };
/// Width of the slide pictures in a PDF.
const PDF_WIDTH: u32 = 1920;

fn even_height(project: &Project, width: u32) -> u32 {
    let (cw, ch) = project.aspect.canvas_size();
    ((f64::from(width) * ch / cw).round() as u32 + 1) & !1
}

fn offscreen(gpu: Option<Gpu>, w: u32, h: u32) -> Result<Offscreen, ExportError> {
    Ok(match gpu {
        Some(gpu) => Offscreen::with_gpu(gpu, w, h)?,
        None => Offscreen::new(w, h)?,
    })
}

/// Writes `out` through a temporary file, so a failed or cancelled export
/// leaves nothing behind.
fn atomically(out: &Path, write: impl FnOnce(&Path) -> Result<(), ExportError>) -> Result<(), ExportError> {
    let tmp = out.with_extension("part");
    let result = write(&tmp).and_then(|()| Ok(fs::rename(&tmp, out)?));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// An endlessly looping GIF. Frames that don't change are merged into one
/// longer frame, which keeps files of mostly-still slides small.
pub fn export_gif(
    project: &Project,
    media: &MediaLibrary,
    gpu: Option<Gpu>,
    out: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), ExportError> {
    let (w, h) = (GIF_WIDTH, even_height(project, GIF_WIDTH));
    let mut gpu = offscreen(gpu, w, h)?;
    atomically(out, |tmp| {
        let mut encoder = gif::Encoder::new(BufWriter::new(File::create(tmp)?), w as u16, h as u16, &[]).map_err(gif_error)?;
        encoder.set_repeat(gif::Repeat::Infinite).map_err(gif_error)?;
        let mut cx = RenderContext::new();
        let total = GIF_FPS.frames_in(project.total_duration());
        let frame_cs = (100 * GIF_FPS.den / GIF_FPS.num) as u16;
        let mut pending: Option<(Vec<u8>, u16)> = None;
        let mut rgba = Vec::new();
        for i in 0..total {
            if cancel.load(Ordering::Relaxed) {
                return Err(ExportError::Cancelled);
            }
            gpu.render(project, media, GIF_FPS.frame_start(i), &mut cx, &mut rgba)?;
            match &mut pending {
                Some((last, delay)) if *last == rgba => *delay += frame_cs,
                _ => {
                    if let Some((mut pixels, delay)) = pending.take() {
                        write_gif_frame(&mut encoder, w, h, &mut pixels, delay)?;
                    }
                    pending = Some((rgba.clone(), frame_cs));
                }
            }
            progress(i + 1, total);
        }
        if let Some((mut pixels, delay)) = pending {
            write_gif_frame(&mut encoder, w, h, &mut pixels, delay)?;
        }
        encoder.into_inner().map_err(gif_error)?.flush()?;
        Ok(())
    })
}

fn write_gif_frame<W: Write>(encoder: &mut gif::Encoder<W>, w: u32, h: u32, rgba: &mut [u8], delay: u16) -> Result<(), ExportError> {
    let mut frame = gif::Frame::from_rgba_speed(w as u16, h as u16, rgba, 10);
    frame.delay = delay;
    encoder.write_frame(&frame).map_err(gif_error)
}

fn gif_error(e: impl std::fmt::Display) -> ExportError {
    ExportError::Io(std::io::Error::other(e.to_string()))
}

/// A PDF with each slide, as it looks once its items have arrived.
pub fn export_pdf(
    project: &Project,
    media: &MediaLibrary,
    gpu: Option<Gpu>,
    out: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), ExportError> {
    let (w, h) = (PDF_WIDTH, even_height(project, PDF_WIDTH));
    let mut gpu = offscreen(gpu, w, h)?;
    let mut cx = RenderContext::new();
    let mut pages = Vec::new();
    let mut rgba = Vec::new();
    let mut start = Tick::ZERO;
    let total = project.slides.len() as u64;
    for (i, slide) in project.slides.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ExportError::Cancelled);
        }
        let at = start + kn_render::rest_time(slide.elements.len()).min(slide.duration - Tick(1));
        gpu.render(project, media, at, &mut cx, &mut rgba)?;
        pages.push(jpeg(&rgba, w, h)?);
        start = start + slide.duration;
        progress(i as u64 + 1, total);
    }
    atomically(out, |tmp| Ok(fs::write(tmp, pdf(&pages, w, h))?))
}

fn jpeg(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, ExportError> {
    let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode(&rgb, w, h, image::ExtendedColorType::Rgb8)
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    Ok(out)
}

/// A minimal PDF: one page per JPEG, the picture filling the page.
fn pdf(pages: &[Vec<u8>], w: u32, h: u32) -> Vec<u8> {
    // 960 points wide (13.3 in), height to match the slide shape.
    let (pw, ph) = (960.0, 960.0 * f64::from(h) / f64::from(w));
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    // Objects: 1 catalog, 2 page tree, then (page, contents, image) per slide.
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 3 + i * 3)).collect();
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(&mut out, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()).as_bytes());
    for (i, jpeg) in pages.iter().enumerate() {
        let (page, contents, img) = (3 + i * 3, 4 + i * 3, 5 + i * 3);
        object(
            &mut out,
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph:.2}] /Contents {contents} 0 R /Resources << /XObject << /Im{page} {img} 0 R >> >> >>").as_bytes(),
        );
        let draw = format!("q {pw} 0 0 {ph:.2} 0 0 cm /Im{page} Do Q");
        object(&mut out, format!("<< /Length {} >>\nstream\n{draw}\nendstream", draw.len()).as_bytes());
        let mut body = format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", jpeg.len()).into_bytes();
        body.extend_from_slice(jpeg);
        body.extend_from_slice(b"\nendstream");
        object(&mut out, &body);
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_has_a_page_per_image_and_a_valid_xref() {
        let pages = vec![vec![0xff, 0xd8, 1, 2, 0xff, 0xd9]; 3];
        let bytes = pdf(&pages, 1920, 1080);
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(text.contains("/Count 3"));
        assert_eq!(text.matches("/Type /Page ").count(), 3);
        // startxref points at the xref table.
        let start: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(bytes[start..].starts_with(b"xref"));
        // Every xref entry points at its object.
        let table = String::from_utf8_lossy(&bytes[start..]).into_owned();
        for (n, line) in table.lines().skip(3).take(11).enumerate() {
            let at: usize = line[..10].parse().unwrap();
            assert!(bytes[at..].starts_with(format!("{} 0 obj", n + 1).as_bytes()), "object {}", n + 1);
        }
    }
}
