//! Markdown to an outline. Headings start slides, lists become points,
//! block quotes become quote slides, `---` forces a break and images
//! (`![caption](file.png)`) become picture slides.

use crate::{Outline, Part, add_picture, strip_bullet};
use kn_media::MediaLibrary;
use std::path::Path;

#[derive(Default)]
struct Draft {
    level: u8,
    title: String,
    lines: Vec<String>,
    points: Vec<String>,
    quote: Vec<String>,
}

impl Draft {
    fn is_empty(&self) -> bool {
        self.title.is_empty() && self.lines.is_empty() && self.points.is_empty() && self.quote.is_empty()
    }

    fn finish(self, parts: &mut Vec<Part>) {
        if self.is_empty() {
            return;
        }
        let body = self.lines.join(" ");
        if !self.quote.is_empty() {
            // A line after the quote that starts with a dash names the author.
            let author = body.trim_start_matches(['—', '–', '-', ' ']).to_owned();
            parts.push(Part::Quote { quote: self.quote.join(" "), author });
        } else if !self.points.is_empty() {
            let title = if self.title.is_empty() { body } else { self.title };
            parts.push(Part::Points { title, points: self.points });
        } else if self.level == 1 {
            parts.push(Part::Title { title: self.title, subtitle: body });
        } else if self.title.is_empty() {
            // Text with no heading: its first sentence becomes the title.
            let (title, rest) = split_first_sentence(&body);
            parts.push(Part::Text { title, body: rest });
        } else {
            parts.push(Part::Text { title: self.title, body });
        }
    }
}

fn split_first_sentence(text: &str) -> (String, String) {
    match text.find(". ") {
        Some(i) if i < 90 => (text[..=i].trim().to_owned(), text[i + 1..].trim().to_owned()),
        _ => (text.to_owned(), String::new()),
    }
}

/// Parses Markdown; image paths are read relative to `base`.
pub fn parse_markdown(text: &str, base: &Path, media: &mut MediaLibrary) -> Outline {
    let mut parts = Vec::new();
    let mut warnings = Vec::new();
    let mut draft = Draft::default();

    for raw in text.lines() {
        let line = raw.trim();
        if let Some((level, heading)) = heading(line) {
            std::mem::take(&mut draft).finish(&mut parts);
            draft = Draft { level, title: strip_inline(heading), ..Draft::default() };
        } else if line == "---" || line == "***" {
            std::mem::take(&mut draft).finish(&mut parts);
        } else if let Some((caption, path)) = image(line) {
            std::mem::take(&mut draft).finish(&mut parts);
            let full = base.join(path);
            match std::fs::read(&full) {
                Ok(bytes) => parts.extend(add_picture(media, bytes, strip_inline(caption), &mut warnings, path)),
                Err(_) => warnings.push(format!("Picture {path} wasn't found next to the file")),
            }
        } else if let Some(q) = line.strip_prefix('>') {
            if !draft.lines.is_empty() || !draft.points.is_empty() {
                std::mem::take(&mut draft).finish(&mut parts);
            }
            draft.quote.push(strip_inline(q.trim()));
        } else if let Some(point) = strip_bullet(raw) {
            draft.points.push(strip_inline(point));
        } else if !line.is_empty() {
            draft.lines.push(strip_inline(line));
        }
    }
    draft.finish(&mut parts);
    Outline { parts, warnings }
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let level = line.chars().take_while(|&c| c == '#').count();
    let rest = line.get(level..)?.strip_prefix(' ')?;
    (1..=6).contains(&level).then(|| (level as u8, rest.trim()))
}

/// `![caption](path)` on a line by itself.
fn image(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("![")?;
    let (caption, rest) = rest.split_once("](")?;
    let path = rest.strip_suffix(')')?;
    let path = path.split_whitespace().next()?; // drop a "title" after the path
    Some((caption, path))
}

/// Removes emphasis and code marks and turns links into their text.
fn strip_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' => {}
            '[' => {
                let text: String = chars.by_ref().take_while(|&c| c != ']').collect();
                if chars.peek() == Some(&'(') {
                    chars.by_ref().take_while(|&c| c != ')').for_each(drop);
                }
                out.push_str(&text);
            }
            _ => out.push(c),
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(md: &str) -> Outline {
        parse_markdown(md, Path::new("."), &mut MediaLibrary::new())
    }

    #[test]
    fn headings_lists_and_quotes() {
        let o = parse(
            "# Launch day\nEverything you need\n\n## Why now\nPeople **love** it. A lot.\n\n## Plan\n- Build\n- [Ship](https://x)\n- Celebrate\n\n> It just works.\n— Sam",
        );
        assert_eq!(
            o.parts,
            vec![
                Part::Title { title: "Launch day".into(), subtitle: "Everything you need".into() },
                Part::Text { title: "Why now".into(), body: "People love it. A lot.".into() },
                Part::Points { title: "Plan".into(), points: vec!["Build".into(), "Ship".into(), "Celebrate".into()] },
                Part::Quote { quote: "It just works.".into(), author: "Sam".into() },
            ]
        );
    }

    #[test]
    fn rule_breaks_slides_and_missing_pictures_warn() {
        let o = parse("First thing. More words\n---\nSecond\n\n![A cat](missing.png)");
        assert_eq!(o.parts.len(), 2);
        assert_eq!(o.parts[0], Part::Text { title: "First thing.".into(), body: "More words".into() });
        assert_eq!(o.warnings.len(), 1);
    }

    #[test]
    fn pictures_next_to_the_file_are_added() {
        let dir = std::env::temp_dir().join(format!("kinetto-md-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.png"), kn_media::test_png(8, 4)).unwrap();
        let mut media = MediaLibrary::new();
        let o = parse_markdown("![Our team](p.png \"title\")", &dir, &mut media);
        assert!(matches!(&o.parts[..], [Part::Picture { width: 8, height: 4, caption, .. }] if caption == "Our team"));
        assert_eq!(media.ids().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
