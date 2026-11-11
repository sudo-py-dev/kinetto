//! Translations and right-to-left text.
//!
//! Strings are looked up by their English text, so the code reads normally
//! and anything without a translation simply stays English. The language is
//! kept per thread and set at the start of every frame from the settings.
//!
//! # Right-to-left text
//!
//! egui has no bidi support. It splits text into runs by font and draws
//! each run in the direction of its first letter. With the usual font
//! setup (Hebrew letters from a fallback font, spaces and digits from the
//! UI font) every Hebrew word is its own run: its letters come out right to
//! left, but the words follow each other left to right, and a full stop
//! lands on the wrong side.
//!
//! So text for display has its *pieces* put into display order here: the
//! words of a line, and inside a word the punctuation or number attached
//! to it. Letters are never reversed. Whether this is needed at all is
//! measured from the fonts ([`probe`]); when a single font draws Hebrew and
//! spaces alike, egui already orders the words and nothing is changed.
//!
//! Display-order text starts with an invisible [`MARK`], so it is never
//! reordered twice and can be turned back when it is put into another
//! sentence ([`format`]) or has to be wrapped (`adw::wrapped`).

mod strings_app;
mod strings_editor;
mod strings_model;
mod strings_quick;

use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One English string and its translations, in the order of
/// [`Language::TRANSLATED`].
pub(crate) type Row = (&'static str, [&'static str; 6]);

/// The languages the editor can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    English,
    Hebrew,
    Russian,
    Chinese,
    French,
    Spanish,
    German,
}

impl Default for Language {
    /// The system's language when Kinetto has it, otherwise English.
    fn default() -> Self {
        // Tests look widgets up by their English names.
        if cfg!(test) {
            return Self::English;
        }
        ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|name| std::env::var(name).ok()).find(|v| !v.is_empty()).map_or(Self::English, |v| Self::from_locale(&v))
    }
}

impl Language {
    /// Every language, English first.
    pub const ALL: [Self; 7] = [Self::English, Self::Hebrew, Self::Russian, Self::Chinese, Self::French, Self::Spanish, Self::German];
    /// The languages with a column in the string tables, in column order.
    const TRANSLATED: [Self; 6] = [Self::Hebrew, Self::Russian, Self::Chinese, Self::French, Self::Spanish, Self::German];

    /// The language's own name for itself, as shown in the language menu.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Hebrew => "עברית",
            Self::Russian => "Русский",
            Self::Chinese => "中文",
            Self::French => "Français",
            Self::Spanish => "Español",
            Self::German => "Deutsch",
        }
    }

    /// Written right to left, so the whole window is mirrored.
    pub fn is_rtl(self) -> bool {
        self == Self::Hebrew
    }

    /// The language for a locale name such as `he_IL.UTF-8` or `zh-CN`.
    pub fn from_locale(locale: &str) -> Self {
        let code: String = locale.chars().take_while(char::is_ascii_alphabetic).collect::<String>().to_ascii_lowercase();
        match code.as_str() {
            "he" | "iw" => Self::Hebrew,
            "ru" => Self::Russian,
            "zh" => Self::Chinese,
            "fr" => Self::French,
            "es" => Self::Spanish,
            "de" => Self::German,
            _ => Self::English,
        }
    }

    /// This language's column in the string tables; `None` for English.
    fn column(self) -> Option<usize> {
        Self::TRANSLATED.iter().position(|l| *l == self)
    }
}

thread_local! {
    /// The language of the window drawn on this thread.
    static CURRENT: Cell<Language> = const { Cell::new(Language::English) };
}

/// Chooses the language for everything drawn on this thread from now on.
pub fn set(language: Language) {
    CURRENT.with(|c| c.set(language));
}

/// The language being shown.
pub fn current() -> Language {
    CURRENT.with(Cell::get)
}

/// The window is mirrored: text starts at the right.
pub fn rtl() -> bool {
    current().is_rtl()
}

/// Every string table, in no particular order.
fn rows() -> impl Iterator<Item = &'static Row> {
    strings_app::ROWS.iter().chain(strings_editor::ROWS).chain(strings_model::ROWS).chain(strings_quick::ROWS)
}

/// One language's strings by their English text, built on first use.
fn table(language: Language) -> Option<&'static HashMap<&'static str, Entry>> {
    static TABLES: OnceLock<Vec<HashMap<&'static str, Entry>>> = OnceLock::new();
    let column = language.column()?;
    let tables = TABLES.get_or_init(|| {
        Language::TRANSLATED
            .iter()
            .enumerate()
            .map(|(column, language)| {
                rows()
                    .filter(|(_, translations)| !translations[column].is_empty())
                    .map(|(english, translations)| {
                        let written = translations[column];
                        // Display order is worked out once and kept for the run.
                        let reordered = if language.is_rtl() { &*marked(written, true).leak() } else { written };
                        (*english, Entry { written, reordered })
                    })
                    .collect()
            })
            .collect()
    });
    tables.get(column)
}

/// A translation as written, and with its pieces in display order.
#[derive(Clone, Copy)]
struct Entry {
    written: &'static str,
    reordered: &'static str,
}

impl Entry {
    /// The form to hand to egui with the fonts in use.
    fn shown(self) -> &'static str {
        if reorders() { self.reordered } else { self.written }
    }
}

/// Starts every string that is already in display order. Invisible in egui.
pub const MARK: char = '\u{2060}';

thread_local! {
    /// Whether the fonts in use need words put into display order; see [`probe`].
    static REORDER: Cell<bool> = const { Cell::new(true) };
}

/// Words must be put into display order for the fonts in use.
fn reorders() -> bool {
    REORDER.with(Cell::get)
}

/// Measures how the fonts in use draw two Hebrew words, and remembers
/// whether the words need reordering. Call once the fonts are installed.
pub fn probe(ctx: &egui::Context) {
    let galley = ctx.fonts_mut(|fonts| fonts.layout_no_wrap("אא בב".to_owned(), egui::FontId::proportional(14.0), egui::Color32::WHITE));
    let x_of = |letter: char| galley.rows.first().and_then(|row| row.glyphs.iter().find(|g| g.chr == letter)).map(|g| g.pos.x);
    // Read right to left, the first word belongs on the right.
    let first_word_is_left = matches!((x_of('א'), x_of('ב')), (Some(first), Some(second)) if first < second);
    REORDER.with(|r| r.set(first_word_is_left));
}

/// `english` in the current language. Text without a translation (and
/// anything already translated) comes back unchanged.
pub fn tr(english: &str) -> &str {
    table(current()).and_then(|t| t.get(english)).map_or(english, |entry| entry.shown())
}

/// `english` in the current language as it is written, for text that
/// becomes part of the video or a file name rather than a label: the
/// sample story, a new text box's words, the window title.
pub fn content(english: &str) -> &str {
    table(current()).and_then(|t| t.get(english)).map_or(english, |entry| entry.written)
}

/// Like [`tr`], for a word that means something else in this place than
/// in most others ("Slide" the page, not the sliding motion). The tables
/// list it as `context|English`; without that row it falls back to [`tr`].
pub fn tr_as<'a>(context: &str, english: &'a str) -> &'a str {
    let key = format!("{context}|{english}");
    table(current()).and_then(|t| t.get(key.as_str())).map_or_else(|| tr(english), |entry| entry.shown())
}

/// Fills `{name}` places in the translated `template` and returns it ready
/// to draw. Values that are already in display order are turned back
/// first, so the sentence is ordered once, as a whole. Use through
/// [`trf!`](crate::trf).
pub fn format(template: &str, values: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut text = table(current()).and_then(|t| t.get(template)).map_or(template, |entry| entry.written).to_owned();
    for (name, value) in values {
        text = text.replace(&format!("{{{name}}}"), &written(&value.to_string()));
    }
    display(&text).into_owned()
}

/// Joins the parts of a label such as "3 slides · Landscape", first part
/// where reading starts, and returns it ready to draw.
pub fn row(parts: &[&str], separator: &str) -> String {
    let written: Vec<_> = parts.iter().map(|part| written(part)).collect();
    if !(reorders() && rtl()) {
        return display(&written.join(separator)).into_owned();
    }
    // Each part is ordered by itself, so "0:01 / 0:21" beside a Hebrew
    // phrase is not pulled into it; then the parts run from the right.
    let ordered: Vec<String> = written.iter().rev().map(|part| reorder(part, true)).collect();
    format!("{MARK}{}", ordered.join(separator))
}

/// Text ready to draw. Left-to-right text in a left-to-right language is
/// returned as it is. Use this for words the user typed (names, slide
/// text); translated strings from [`tr`] and [`trf!`](crate::trf) already are.
pub fn display(text: &str) -> std::borrow::Cow<'_, str> {
    let rtl = rtl();
    if !reorders() || text.starts_with(MARK) || !(rtl || text.chars().any(is_rtl_char)) {
        return text.into();
    }
    marked(text, rtl).into()
}

/// Text as it is written, whether or not it was put into display order.
pub fn written(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.starts_with(MARK) {
        return text.into();
    }
    // Reordering twice gives back the original; the language's direction is
    // the one the text was ordered with.
    let plain: String = text.chars().filter(|c| *c != MARK).collect();
    reorder(&plain, rtl()).into()
}

/// `text` in display order, with the mark that says so.
fn marked(text: &str, rtl_base: bool) -> String {
    format!("{MARK}{}", reorder(text, rtl_base))
}

/// A letter from a right-to-left script (Hebrew, Arabic and their relatives).
fn is_rtl_char(c: char) -> bool {
    matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// How a word reads.
#[derive(Clone, Copy, PartialEq)]
enum Reads {
    /// It has right-to-left letters.
    Rtl,
    /// Latin letters or digits only.
    Ltr,
    /// Only punctuation or symbols; it goes with its neighbours.
    Neutral,
}

/// Classifies a word by its letters.
fn reads(word: &str) -> Reads {
    if word.chars().any(is_rtl_char) {
        Reads::Rtl
    } else if word.chars().any(char::is_alphanumeric) {
        Reads::Ltr
    } else {
        Reads::Neutral
    }
}

/// Puts the pieces of each line into display order. Applying it twice
/// gives the original back.
///
/// With a right-to-left base the words are laid out from the right, except
/// that a stretch of left-to-right words ("Welcome to Kinetto", "0:01 /
/// 0:21") keeps its own order. With a left-to-right base only stretches of
/// right-to-left words are turned. Inside a right-to-left word, the parts
/// that are not letters (a full stop, an attached number) change sides.
///
/// This is a small part of the Unicode bidi algorithm, enough for interface
/// text. One known gap: punctuation after a left-to-right word that ends a
/// right-to-left sentence stays with that word.
fn reorder(text: &str, rtl_base: bool) -> String {
    text.split('\n').map(|line| reorder_line(line, rtl_base)).collect::<Vec<_>>().join("\n")
}

/// [`reorder`] for one line.
fn reorder_line(line: &str, rtl_base: bool) -> String {
    // Words and the gaps between them, alternating, starting with a word
    // (possibly empty).
    let mut pieces: Vec<&str> = Vec::new();
    let mut rest = line;
    loop {
        let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        pieces.push(&rest[..word_end]);
        rest = &rest[word_end..];
        if rest.is_empty() {
            break;
        }
        let gap_end = rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
        pieces.push(&rest[..gap_end]);
        rest = &rest[gap_end..];
    }
    let words: Vec<usize> = (0..pieces.len()).step_by(2).collect();
    let kinds: Vec<Reads> = words.iter().map(|&i| reads(pieces[i])).collect();
    // A neutral word between two words of the same kind belongs with them.
    let joins = |n: usize, kind: Reads| {
        kinds[n] == kind || (kinds[n] == Reads::Neutral && n > 0 && n + 1 < kinds.len() && kinds[..n].iter().rev().find(|k| **k != Reads::Neutral) == Some(&kind) && kinds[n + 1..].iter().find(|k| **k != Reads::Neutral) == Some(&kind))
    };
    // Groups of piece indices that stay together, in reading order.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut n = 0;
    while n < words.len() {
        // The kind of word that forms stretches: against the line's direction.
        let together = if rtl_base { Reads::Ltr } else { Reads::Rtl };
        let mut end = n + 1;
        if joins(n, together) {
            while end < words.len() && joins(end, together) {
                end += 1;
            }
        }
        // The words of the group with the gaps between them.
        groups.push((words[n]..=words[end - 1]).collect());
        if end < words.len() {
            groups.push(vec![words[end] - 1]);
        }
        n = end;
    }
    let word = |i: usize| if reads(pieces[i]) == Reads::Rtl { reorder_word(pieces[i]) } else { pieces[i].to_owned() };
    let mut out = String::with_capacity(line.len());
    if rtl_base {
        // The whole line runs from the right; left-to-right stretches keep their order.
        for group in groups.iter().rev() {
            for &i in group {
                out.push_str(&word(i));
            }
        }
    } else {
        // Only the right-to-left stretches are turned.
        for group in &groups {
            let turn = group.len() > 1 && group.iter().step_by(2).any(|&i| reads(pieces[i]) == Reads::Rtl);
            if turn {
                for &i in group.iter().rev() {
                    out.push_str(&word(i));
                }
            } else {
                for &i in group {
                    out.push_str(&word(i));
                }
            }
        }
    }
    out
}

/// Turns the parts of a right-to-left word: letters stay as written, and
/// whatever is attached to them (punctuation, a number) changes sides,
/// with brackets facing the other way.
fn reorder_word(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        if is_rtl_char(chars[i]) {
            while i < chars.len() && is_rtl_char(chars[i]) {
                i += 1;
            }
            parts.push(chars[start..i].iter().collect());
        } else if chars[i].is_alphanumeric() {
            // A number or Latin word, with what joins it up inside ("0:07", "Ctrl+O").
            let more = |k: usize| k < chars.len() && !is_rtl_char(chars[k]) && (chars[k].is_alphanumeric() || (k + 1 < chars.len() && chars[k + 1].is_alphanumeric() && !is_rtl_char(chars[k + 1])));
            while more(i) {
                i += 1;
            }
            parts.push(chars[start..i].iter().collect());
        } else {
            parts.push(mirrored(chars[i]).to_string());
            i += 1;
        }
    }
    // Every part keeps its own inside order; the parts change places.
    parts.reverse();
    parts.concat()
}

/// The bracket facing the other way.
fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        other => other,
    }
}

/// Translates a string with values in it:
/// `trf!("Slide {n} of {total}", n = 2, total = 5)`.
#[macro_export]
macro_rules! trf {
    ($template:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::format($template, &[$((stringify!($name), &$value as &dyn std::fmt::Display)),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_map_to_languages() {
        assert_eq!(Language::from_locale("he_IL.UTF-8"), Language::Hebrew);
        assert_eq!(Language::from_locale("zh-CN"), Language::Chinese);
        assert_eq!(Language::from_locale("ru"), Language::Russian);
        assert_eq!(Language::from_locale("C"), Language::English);
        assert_eq!(Language::from_locale(""), Language::English);
    }

    #[test]
    fn untranslated_text_and_english_stay_as_written() {
        set(Language::English);
        assert_eq!(tr("Export"), "Export");
        set(Language::French);
        assert_eq!(tr("no such string in any table"), "no such string in any table");
        assert_eq!(tr("Export"), "Exporter");
        // A translation is not looked up again as if it were English.
        assert_eq!(tr(tr("Export")), "Exporter");
        set(Language::English);
    }

    #[test]
    fn words_are_put_in_display_order_and_letters_are_left_alone() {
        // Read right to left, the result starts with the first word.
        assert_eq!(reorder("סרטון ללא שם", true), "שם ללא סרטון");
        // A full stop changes sides; a stretch of English keeps its order.
        assert_eq!(reorder("ברוכים הבאים אל Welcome to Kinetto", true), "Welcome to Kinetto אל הבאים ברוכים");
        assert_eq!(reorder("גררו כדי לסובב.", true), ".לסובב כדי גררו");
        assert_eq!(reorder("שקף 2 מתוך 5", true), "5 מתוך 2 שקף");
        assert_eq!(reorder("0:01 / 0:21", true), "0:01 / 0:21");
        assert_eq!(reorder("מתחיל ב-0:07", true), "0:07-ב מתחיל");
        assert_eq!(reorder("פתיחה (Ctrl+O)", true), "(Ctrl+O) פתיחה");
        // In an English sentence only the Hebrew stretch is turned.
        assert_eq!(reorder("Open שלום עולם now", false), "Open עולם שלום now");
        for (text, rtl) in [("סרטון ללא שם", true), ("ברוכים הבאים אל Welcome to Kinetto", true), ("שקף 2 מתוך 5  ·  0:01 / 0:21", true), ("Open שלום עולם now", false), ("שורה אחת\nושורה שנייה.", true)] {
            assert_eq!(reorder(&reorder(text, rtl), rtl), text, "reordering twice gives the text back");
        }
    }

    #[test]
    fn display_order_text_is_marked_and_never_ordered_twice() {
        set(Language::Hebrew);
        REORDER.with(|r| r.set(true));
        let shown = tr("Slide settings");
        assert!(shown.starts_with(MARK));
        assert_eq!(display(shown), shown);
        assert_eq!(written(shown), "הגדרות השקף");
        // A translated value inside a sentence is ordered with the sentence.
        let sentence = crate::trf!("New slide: {name}", name = tr("Title and text"));
        assert_eq!(written(&sentence), "שקף חדש: כותרת וטקסט");
        // With fonts that already order Hebrew words, nothing is touched.
        REORDER.with(|r| r.set(false));
        assert_eq!(tr("Slide settings"), "הגדרות השקף");
        REORDER.with(|r| r.set(true));
        set(Language::English);
    }

    #[test]
    fn values_fill_the_translated_template() {
        set(Language::Hebrew);
        assert_eq!(written(&crate::trf!("Slide {n} of {total}", n = 2, total = 5)), "שקף 2 מתוך 5");
        set(Language::English);
        assert_eq!(crate::trf!("Slide {n} of {total}", n = 2, total = 5), "Slide 2 of 5");
    }

    #[test]
    fn a_word_can_have_its_own_translation_in_one_place() {
        set(Language::Hebrew);
        assert_eq!(written(tr("Slide")), "החלקה");
        assert_eq!(written(tr_as("tab", "Slide")), "שקף");
        assert_eq!(written(tr_as("no such context", "Export")), "ייצוא");
        set(Language::English);
        assert_eq!(tr_as("tab", "Slide"), "Slide");
    }

    #[test]
    fn every_row_is_unique_and_keeps_its_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for (english, translations) in rows() {
            assert!(seen.insert(*english), "{english:?} is in the tables twice");
            let places = |s: &str| {
                let mut found: Vec<String> = s.match_indices('{').filter_map(|(i, _)| s[i..].find('}').map(|end| s[i..=i + end].to_owned())).collect();
                found.sort();
                found
            };
            for (column, translation) in translations.iter().enumerate() {
                assert!(!translation.is_empty(), "{english:?} has no {:?} translation", Language::TRANSLATED[column]);
                let english = english.split_once('|').map_or(*english, |(_, word)| word);
                assert_eq!(places(english), places(translation), "{english:?} in {:?}", Language::TRANSLATED[column]);
            }
        }
    }
}
