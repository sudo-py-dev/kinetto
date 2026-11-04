//! PowerPoint (.pptx) to an outline: slide order, titles, text and pictures.
//! Formatting and animations are not carried over; the chosen look and
//! Kinetto's own motion are applied instead.

use crate::{ImportError, Outline, Part, add_picture};
use kn_media::{MAX_FILE_BYTES, MediaLibrary};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Decks with more slides than this are cut short, with a warning.
const MAX_SLIDES: usize = 300;

type Zip = ZipArchive<Cursor<Vec<u8>>>;

pub fn parse_pptx(bytes: Vec<u8>, media: &mut MediaLibrary) -> Result<Outline, ImportError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| ImportError::Damaged(e.to_string()))?;
    let presentation = read_text(&mut zip, "ppt/presentation.xml")?;
    let rels = relationships(&read_text(&mut zip, "ppt/_rels/presentation.xml.rels")?);
    let order = slide_ids(&presentation);

    let mut outline = Outline::default();
    if order.len() > MAX_SLIDES {
        outline.warnings.push(format!("Only the first {MAX_SLIDES} slides were brought in"));
    }
    for (n, rid) in order.iter().take(MAX_SLIDES).enumerate() {
        let Some(target) = rels.get(rid) else { continue };
        let path = resolve("ppt", target);
        let Ok(xml) = read_text(&mut zip, &path) else {
            outline.warnings.push(format!("Slide {} couldn't be read", n + 1));
            continue;
        };
        let slide = parse_slide(&xml);
        let rels_path = match path.rsplit_once('/') {
            Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
            None => continue,
        };
        let slide_rels = read_text(&mut zip, &rels_path).map(|x| relationships(&x)).unwrap_or_default();
        let dir = path.rsplit_once('/').map_or("ppt/slides", |(d, _)| d).to_owned();
        let pictures: Vec<Vec<u8>> = slide
            .pictures
            .iter()
            .filter_map(|rid| slide_rels.get(rid))
            .filter_map(|target| read_bytes(&mut zip, &resolve(&dir, target)).ok())
            .collect();
        to_parts(slide, pictures, media, &mut outline, n + 1);
    }
    if outline.parts.is_empty() {
        return Err(ImportError::Empty);
    }
    Ok(outline)
}

/// Text found on one slide, by kind of placeholder.
#[derive(Default, Debug)]
struct SlideText {
    title: String,
    subtitle: String,
    body: Vec<String>,
    /// Relationship ids of the slide's pictures.
    pictures: Vec<String>,
}

fn to_parts(slide: SlideText, pictures: Vec<Vec<u8>>, media: &mut MediaLibrary, outline: &mut Outline, number: usize) {
    let SlideText { title, subtitle, body, .. } = slide;
    let has_text = !title.is_empty() || !subtitle.is_empty() || !body.is_empty();
    // A slide that is mostly a picture becomes a picture slide, captioned
    // with its title.
    if !pictures.is_empty() && body.is_empty() {
        let mut caption = if title.is_empty() { subtitle.clone() } else { title.clone() };
        for (i, bytes) in pictures.into_iter().enumerate() {
            let name = format!("{} on slide {number}", i + 1);
            outline.parts.extend(add_picture(media, bytes, std::mem::take(&mut caption), &mut outline.warnings, &name));
        }
        return;
    }
    if !pictures.is_empty() {
        outline.warnings.push(format!("Slide {number} had text and pictures; its pictures were added as their own slides"));
    }
    if has_text {
        let part = if body.is_empty() {
            Part::Title { title, subtitle }
        } else if body.len() >= 2 && body.iter().all(|l| l.chars().count() <= 80) {
            Part::Points { title, points: body }
        } else {
            Part::Text { title, body: body.join(" ") }
        };
        outline.parts.push(part);
    }
    for (i, bytes) in pictures.into_iter().enumerate() {
        let name = format!("{} on slide {number}", i + 1);
        outline.parts.extend(add_picture(media, bytes, String::new(), &mut outline.warnings, &name));
    }
}

fn parse_slide(xml: &str) -> SlideText {
    #[derive(Clone, Copy, PartialEq)]
    enum Holder {
        Title,
        Subtitle,
        Body,
    }
    let mut out = SlideText::default();
    let mut reader = Reader::from_str(xml);
    let mut in_shape = false;
    let mut holder = Holder::Body;
    let mut paragraph = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                "sp" => {
                    in_shape = true;
                    holder = Holder::Body;
                }
                "p" if in_shape => paragraph.clear(),
                "t" if in_shape => in_text = true,
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                "ph" if in_shape => {
                    holder = match attr(&e, "type").as_deref() {
                        Some("title" | "ctrTitle") => Holder::Title,
                        Some("subTitle") => Holder::Subtitle,
                        _ => Holder::Body,
                    }
                }
                "blip" => out.pictures.extend(attr(&e, "embed")),
                "br" if in_shape => paragraph.push(' '),
                _ => {}
            },
            Ok(Event::Text(t)) if in_text => paragraph.push_str(&t.xml10_content()),
            Ok(Event::GeneralRef(r)) if in_text => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    paragraph.push(c);
                } else {
                    paragraph.push_str(match r.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                "t" => in_text = false,
                "p" if in_shape => {
                    let line = paragraph.trim().to_owned();
                    if !line.is_empty() {
                        match holder {
                            Holder::Title => join(&mut out.title, &line),
                            Holder::Subtitle => join(&mut out.subtitle, &line),
                            Holder::Body => out.body.push(line),
                        }
                    }
                    paragraph.clear();
                }
                "sp" => in_shape = false,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

fn join(target: &mut String, line: &str) {
    if !target.is_empty() {
        target.push(' ');
    }
    target.push_str(line);
}

/// The value of attribute `local` (any prefix), unescaped.
fn attr(e: &BytesStart<'_>, local: &str) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.local_name().as_ref() == local).and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
}

/// Relationship id → target path.
fn relationships(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "Relationship" => {
                if let (Some(id), Some(target)) = (attr(&e, "Id"), attr(&e, "Target")) {
                    map.insert(id, target);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Relationship ids of the slides, in presentation order.
fn slide_ids(xml: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "sldId" => ids.extend(rid(&e)),
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    ids
}

/// The `r:id` attribute (as opposed to the plain numeric `id`).
fn rid(e: &BytesStart<'_>) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref().ends_with(":id")).and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
}

/// Joins a relationship target onto the folder of the part that names it,
/// resolving `..`; absolute targets start from the package root.
fn resolve(dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if let Some(abs) = target.strip_prefix('/') {
        return abs.to_owned();
    } else {
        dir.split('/').filter(|p| !p.is_empty()).collect()
    };
    for piece in target.split('/') {
        match piece {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn read_bytes(zip: &mut Zip, name: &str) -> Result<Vec<u8>, ImportError> {
    let entry = zip.by_name(name).map_err(|e| ImportError::Damaged(format!("{name}: {e}")))?;
    let mut out = Vec::new();
    entry.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut out).map_err(|e| ImportError::Damaged(e.to_string()))?;
    if out.len() > MAX_FILE_BYTES {
        return Err(ImportError::Damaged(format!("{name} is too large")));
    }
    Ok(out)
}

fn read_text(zip: &mut Zip, name: &str) -> Result<String, ImportError> {
    String::from_utf8(read_bytes(zip, name)?).map_err(|_| ImportError::Damaged(format!("{name} isn't text")))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn slide_xml(shapes: &str) -> String {
        format!(r#"<?xml version="1.0"?><p:sld xmlns:p="p" xmlns:a="a" xmlns:r="r"><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"#)
    }

    fn shape(ph: Option<&str>, paragraphs: &[&str]) -> String {
        let ph = ph.map_or(String::new(), |t| format!(r#"<p:nvSpPr><p:nvPr><p:ph type="{t}"/></p:nvPr></p:nvSpPr>"#));
        let body: String = paragraphs.iter().map(|p| format!("<a:p><a:r><a:t>{p}</a:t></a:r></a:p>")).collect();
        format!("<p:sp>{ph}<p:txBody>{body}</p:txBody></p:sp>")
    }

    /// A minimal deck: slides in the given order, plus one picture.
    pub(crate) fn deck(slides: &[String], picture: Option<Vec<u8>>) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        let ids: String = (0..slides.len()).map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, 10 + i)).collect();
        zip.start_file("ppt/presentation.xml", opt).unwrap();
        write!(zip, r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>"#).unwrap();
        let rels: String = (0..slides.len()).map(|i| format!(r#"<Relationship Id="rId{}" Target="slides/slide{}.xml"/>"#, 10 + i, i + 1)).collect();
        zip.start_file("ppt/_rels/presentation.xml.rels", opt).unwrap();
        write!(zip, "<Relationships>{rels}</Relationships>").unwrap();
        for (i, xml) in slides.iter().enumerate() {
            zip.start_file(format!("ppt/slides/slide{}.xml", i + 1), opt).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
            zip.start_file(format!("ppt/slides/_rels/slide{}.xml.rels", i + 1), opt).unwrap();
            write!(zip, r#"<Relationships><Relationship Id="rIdPic" Target="../media/image1.png"/></Relationships>"#).unwrap();
        }
        if let Some(png) = picture {
            zip.start_file("ppt/media/image1.png", opt).unwrap();
            zip.write_all(&png).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_titles_points_entities_and_pictures_in_order() {
        let pic = r#"<p:pic><p:blipFill><a:blip r:embed="rIdPic"/></p:blipFill></p:pic>"#;
        let slides = vec![
            slide_xml(&(shape(Some("ctrTitle"), &["Q3 review"]) + &shape(Some("subTitle"), &["Sales &amp; growth"]))),
            slide_xml(&(shape(Some("title"), &["Highlights"]) + &shape(None, &["Revenue up", "New office", "Hired 5"]))),
            slide_xml(&(shape(Some("title"), &["Our team"]) + pic)),
        ];
        let mut media = MediaLibrary::new();
        let o = parse_pptx(deck(&slides, Some(kn_media::test_png(4, 4))), &mut media).unwrap();
        assert_eq!(o.parts[0], Part::Title { title: "Q3 review".into(), subtitle: "Sales & growth".into() });
        assert_eq!(o.parts[1], Part::Points { title: "Highlights".into(), points: vec!["Revenue up".into(), "New office".into(), "Hired 5".into()] });
        assert!(matches!(&o.parts[2], Part::Picture { caption, .. } if caption == "Our team"));
        assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    }

    #[test]
    fn rejects_non_decks() {
        assert!(parse_pptx(b"not a zip".to_vec(), &mut MediaLibrary::new()).is_err());
        assert!(matches!(parse_pptx(deck(&[slide_xml("")], None), &mut MediaLibrary::new()), Err(ImportError::Empty)));
    }

    #[test]
    fn resolves_relative_targets() {
        assert_eq!(resolve("ppt/slides", "../media/a.png"), "ppt/media/a.png");
        assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("ppt/slides", "/ppt/media/b.png"), "ppt/media/b.png");
    }
}
