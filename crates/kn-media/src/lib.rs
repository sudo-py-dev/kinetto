//! Pictures used by a project. Files are kept as their original bytes and
//! named by content hash, so the same picture added twice is stored once.

use image::{ImageFormat, ImageReader, Limits};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

/// Largest file accepted.
pub const MAX_FILE_BYTES: usize = 100 * 1024 * 1024;
/// Largest width or height accepted.
pub const MAX_SIDE: u32 = 16_384;
/// Decoded pictures are scaled down to fit this, which is enough for 4K.
pub const DECODE_SIDE: u32 = 4096;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("this picture type isn't supported; use PNG, JPEG, WebP or GIF")]
    Unsupported,
    #[error("this picture is too big (over 100 MB or {MAX_SIDE} pixels wide)")]
    TooBig,
    #[error("this picture couldn't be read: {0}")]
    Corrupt(#[from] image::ImageError),
    #[error("picture {0} is missing from the project")]
    Missing(String),
}

/// Basic facts about an imported picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfo {
    pub id: String,
    pub width: u32,
    pub height: u32,
}

/// Straight-alpha RGBA8 pixels.
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Default, Clone)]
pub struct MediaLibrary {
    items: HashMap<String, Arc<[u8]>>,
}

impl MediaLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks `bytes` is a picture we can use and adds it.
    pub fn import(&mut self, bytes: Vec<u8>) -> Result<MediaInfo, MediaError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(MediaError::TooBig);
        }
        let format = image::guess_format(&bytes).map_err(|_| MediaError::Unsupported)?;
        let ext = match format {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::WebP => "webp",
            ImageFormat::Gif => "gif",
            _ => return Err(MediaError::Unsupported),
        };
        let (width, height) = reader(&bytes)?.into_dimensions()?;
        if width == 0 || height == 0 {
            return Err(MediaError::Unsupported);
        }
        if width > MAX_SIDE || height > MAX_SIDE {
            return Err(MediaError::TooBig);
        }
        let hash = blake3::hash(&bytes).to_hex();
        let id = format!("{}.{ext}", &hash[..32]);
        self.items.entry(id.clone()).or_insert_with(|| bytes.into());
        Ok(MediaInfo { id, width, height })
    }

    /// Adds a sound file. The format is recognised from its first bytes;
    /// decoding is checked by the audio engine before this is called.
    pub fn import_audio(&mut self, bytes: Vec<u8>) -> Result<String, MediaError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(MediaError::TooBig);
        }
        let ext = audio_extension(&bytes).ok_or(MediaError::Unsupported)?;
        let hash = blake3::hash(&bytes).to_hex();
        let id = format!("{}.{ext}", &hash[..32]);
        self.items.entry(id.clone()).or_insert_with(|| bytes.into());
        Ok(id)
    }

    /// Adds bytes read back from a saved project.
    pub fn insert(&mut self, id: String, bytes: Vec<u8>) {
        self.items.insert(id, bytes.into());
    }

    /// Transfers already-loaded media without copying a large file on the UI thread.
    pub fn insert_shared(&mut self, id: String, bytes: Arc<[u8]>) {
        self.items.insert(id, bytes);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<[u8]>> {
        self.items.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.items.keys().map(String::as_str)
    }

    /// Decodes a picture to RGBA, scaled to fit [`DECODE_SIDE`].
    pub fn decode(&self, id: &str) -> Result<Decoded, MediaError> {
        let bytes = self.get(id).ok_or_else(|| MediaError::Missing(id.to_owned()))?;
        let mut img = reader(bytes)?.decode()?;
        if img.width() > DECODE_SIDE || img.height() > DECODE_SIDE {
            img = img.resize(DECODE_SIDE, DECODE_SIDE, image::imageops::FilterType::Triangle);
        }
        let rgba = img.into_rgba8();
        Ok(Decoded { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
    }

    /// A quick, small copy of a picture that fits in `side` pixels.
    pub fn thumbnail(&self, id: &str, side: u32) -> Result<Decoded, MediaError> {
        let bytes = self.get(id).ok_or_else(|| MediaError::Missing(id.to_owned()))?;
        let img = reader(bytes)?.decode()?.thumbnail(side, side);
        let rgba = img.into_rgba8();
        Ok(Decoded { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
    }
}

/// Extensions of the sound files a project can hold.
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "flac", "ogg", "mp3", "m4a"];

fn audio_extension(b: &[u8]) -> Option<&'static str> {
    match b {
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'A', b'V', b'E', ..] => Some("wav"),
        [b'f', b'L', b'a', b'C', ..] => Some("flac"),
        [b'O', b'g', b'g', b'S', ..] => Some("ogg"),
        [b'I', b'D', b'3', ..] => Some("mp3"),
        [0xff, second, ..] if second & 0xe0 == 0xe0 => Some("mp3"),
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => Some("m4a"),
        _ => None,
    }
}

/// A reader with memory limits, so a hostile file can't exhaust memory.
fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, MediaError> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|_| MediaError::Unsupported)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(1024 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

/// A small generated PNG, for tests in this and other crates.
#[doc(hidden)]
pub fn test_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba([x as u8, y as u8, 128, 255]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).expect("encoding a PNG in memory");
    out.into_inner()
}

/// Original, bundled illustrations. No network request is needed to use them.
pub const SAMPLE_ART: [(&str, &[u8]); 8] = [
    ("Quiet landscape", include_bytes!("../../../assets/samples/quiet-landscape.png")),
    ("Studio garden", include_bytes!("../../../assets/samples/studio-garden.png")),
    ("Orbit study", include_bytes!("../../../assets/samples/orbit-study.png")),
    ("Birthday cake", include_bytes!("../../../assets/samples/birthday-cake.png")),
    ("Party balloons", include_bytes!("../../../assets/samples/party-balloons.png")),
    ("Seaside", include_bytes!("../../../assets/samples/seaside.png")),
    ("City lights", include_bytes!("../../../assets/samples/city-lights.png")),
    ("Wildflowers", include_bytes!("../../../assets/samples/wildflowers.png")),
];

pub fn sample_id(index: usize) -> String {
    let hash = blake3::hash(SAMPLE_ART[index].1).to_hex();
    format!("{}.png", &hash[..32])
}

pub fn sample_library() -> MediaLibrary {
    let mut media = MediaLibrary::new();
    for (i, (_, bytes)) in SAMPLE_ART.iter().enumerate() {
        media.insert(sample_id(i), bytes.to_vec());
    }
    media
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_dedupes_by_content() {
        let mut lib = MediaLibrary::new();
        let a = lib.import(test_png(4, 3)).unwrap();
        let b = lib.import(test_png(4, 3)).unwrap();
        assert_eq!(a, b);
        assert_eq!((a.width, a.height), (4, 3));
        assert!(a.id.ends_with(".png"));
        assert_eq!(lib.ids().count(), 1);
    }

    #[test]
    fn rejects_non_images() {
        let mut lib = MediaLibrary::new();
        assert!(matches!(lib.import(b"hello, not a picture".to_vec()), Err(MediaError::Unsupported)));
        assert_eq!(lib.ids().count(), 0);
    }

    #[test]
    fn decode_gives_rgba() {
        let mut lib = MediaLibrary::new();
        let info = lib.import(test_png(5, 2)).unwrap();
        let d = lib.decode(&info.id).unwrap();
        assert_eq!((d.width, d.height, d.rgba.len()), (5, 2, 5 * 2 * 4));
        assert!(matches!(lib.decode("nope.png"), Err(MediaError::Missing(_))));
    }
}
