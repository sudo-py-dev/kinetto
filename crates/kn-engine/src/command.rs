//! Every change to a project is a [`Command`]. Commands have stable ids
//! (`slide.add`, `element.update`, ...) so the same list drives the UI, undo,
//! scripts and, later, the CLI and MCP server.

use kn_geom::{Aspect, Color, Rect};
use kn_layout::{Layout, Words};
use kn_model::{Element, Music, Project, Slide, Transition};
use kn_time::Tick;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "id")]
pub enum Command {
    #[serde(rename = "slide.add")]
    AddSlide { at: usize },
    #[serde(rename = "slide.duplicate")]
    DuplicateSlide { index: usize },
    #[serde(rename = "slide.delete")]
    DeleteSlide { index: usize },
    #[serde(rename = "slide.move")]
    MoveSlide { from: usize, to: usize },
    #[serde(rename = "slide.setBackground")]
    SetBackground { index: usize, color: Color },
    #[serde(rename = "slide.setDuration")]
    SetDuration { index: usize, millis: i64 },
    #[serde(rename = "slide.setTransition")]
    SetTransition { index: usize, transition: Transition },
    #[serde(rename = "element.add")]
    AddElement { slide: usize, element: Element },
    #[serde(rename = "element.delete")]
    DeleteElement { slide: usize, index: usize },
    #[serde(rename = "element.setFrame")]
    SetFrame { slide: usize, index: usize, frame: Rect },
    #[serde(rename = "element.update")]
    UpdateElement { slide: usize, index: usize, element: Element },
    #[serde(rename = "element.reorder")]
    ReorderElement { slide: usize, from: usize, to: usize },
    #[serde(rename = "project.setAspect")]
    SetAspect { aspect: Aspect },
    #[serde(rename = "project.applyTheme")]
    ApplyTheme { theme: String },
    #[serde(rename = "slide.addLayout")]
    AddLayoutSlide { at: usize, layout: Layout },
    #[serde(rename = "slide.setDark")]
    SetDark { index: usize, dark: bool },
    #[serde(rename = "project.setMusic")]
    SetMusic { music: Option<Music> },
    /// Sets the slide's voice; the slide grows to at least `fit_millis` so
    /// the voice is never cut off.
    #[serde(rename = "slide.setVoice")]
    SetVoice { index: usize, voice: Option<String>, fit_millis: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("there is no slide {0}")]
    NoSlide(usize),
    #[error("slide {slide} has no element {index}")]
    NoElement { slide: usize, index: usize },
    #[error("a project needs at least one slide")]
    LastSlide,
    #[error("a slide must last between 0.5 s and 60 s")]
    BadDuration,
}

/// Shortest and longest slide, in milliseconds.
const DURATION_RANGE: std::ops::RangeInclusive<i64> = 500..=60_000;

impl Command {
    /// Plain-language name for menus and the undo history.
    pub fn label(&self) -> &'static str {
        match self {
            Command::AddSlide { .. } => "Add slide",
            Command::DuplicateSlide { .. } => "Duplicate slide",
            Command::DeleteSlide { .. } => "Delete slide",
            Command::MoveSlide { .. } => "Move slide",
            Command::SetBackground { .. } => "Change background",
            Command::SetDuration { .. } => "Change slide length",
            Command::SetTransition { .. } => "Change transition",
            Command::AddElement { .. } => "Add item",
            Command::DeleteElement { .. } => "Delete item",
            Command::SetFrame { .. } => "Move item",
            Command::UpdateElement { .. } => "Edit item",
            Command::ReorderElement { .. } => "Reorder item",
            Command::SetAspect { .. } => "Change shape",
            Command::ApplyTheme { .. } => "Change look",
            Command::AddLayoutSlide { .. } => "Add slide",
            Command::SetDark { .. } => "Change slide style",
            Command::SetMusic { .. } => "Change music",
            Command::SetVoice { .. } => "Change voice",
        }
    }

    /// Commands with the same key that arrive back to back merge into one
    /// undo step, so a drag or a burst of typing undoes in one go.
    pub(crate) fn coalesce_key(&self) -> Option<(&'static str, usize, usize)> {
        match self {
            Command::SetFrame { slide, index, .. } => Some(("frame", *slide, *index)),
            Command::UpdateElement { slide, index, .. } => Some(("update", *slide, *index)),
            Command::SetBackground { index, .. } => Some(("background", *index, 0)),
            Command::SetMusic { .. } => Some(("music", 0, 0)),
            Command::SetDuration { index, .. } => Some(("duration", *index, 0)),
            _ => None,
        }
    }

    /// Returns the project with this command applied; `project` is untouched.
    pub fn apply(&self, project: &Project) -> Result<Project, CommandError> {
        let mut p = project.clone();
        match self {
            Command::AddSlide { at } => {
                let blank = p.blank_slide();
                p.slides.insert((*at).min(p.slides.len()), blank);
            }
            Command::DuplicateSlide { index } => {
                let copy = slide(&p, *index)?.clone();
                p.slides.insert(index + 1, copy);
            }
            Command::DeleteSlide { index } => {
                slide(&p, *index)?;
                if p.slides.len() == 1 {
                    return Err(CommandError::LastSlide);
                }
                p.slides.remove(*index);
            }
            Command::MoveSlide { from, to } => {
                slide(&p, *from)?;
                slide(&p, *to)?;
                let s = p.slides.remove(*from);
                p.slides.insert(*to, s);
            }
            Command::SetBackground { index, color } => slide_mut(&mut p, *index)?.background = *color,
            Command::SetDuration { index, millis } => {
                if !DURATION_RANGE.contains(millis) {
                    return Err(CommandError::BadDuration);
                }
                slide_mut(&mut p, *index)?.duration = Tick::from_millis(*millis);
            }
            Command::SetTransition { index, transition } => slide_mut(&mut p, *index)?.transition = *transition,
            Command::AddElement { slide, element } => slide_mut(&mut p, *slide)?.elements.push(element.clone()),
            Command::DeleteElement { slide, index } => {
                element(&p, *slide, *index)?;
                slide_mut(&mut p, *slide)?.elements.remove(*index);
            }
            Command::SetFrame { slide, index, frame } => element_mut(&mut p, *slide, *index)?.frame = *frame,
            Command::UpdateElement { slide, index, element } => *element_mut(&mut p, *slide, *index)? = element.clone(),
            Command::ReorderElement { slide, from, to } => {
                element(&p, *slide, *from)?;
                element(&p, *slide, *to)?;
                let elements = &mut slide_mut(&mut p, *slide)?.elements;
                let e = elements.remove(*from);
                elements.insert(*to, e);
            }
            // Items move so each keeps its place on the new shape.
            Command::SetAspect { aspect } => p = kn_layout::reflow(&p, *aspect),
            Command::ApplyTheme { theme } => p = kn_layout::apply_theme(&p, kn_layout::theme(theme)),
            Command::AddLayoutSlide { at, layout } => {
                let new = kn_layout::slide(*layout, Words::default(), p.aspect, kn_layout::theme(&p.theme));
                p.slides.insert((*at).min(p.slides.len()), new);
            }
            Command::SetMusic { music } => p.music = music.clone(),
            Command::SetVoice { index, voice, fit_millis } => {
                let slide = slide_mut(&mut p, *index)?;
                slide.voice = voice.clone();
                let fit = Tick::from_millis((*fit_millis).clamp(0, *DURATION_RANGE.end()));
                if fit > slide.duration {
                    slide.duration = fit;
                }
            }
            Command::SetDark { index, dark } => {
                slide_mut(&mut p, *index)?.dark = *dark;
                p = kn_layout::recolour(&p, kn_layout::theme(&p.theme));
            }
        }
        Ok(p)
    }
}

fn slide(p: &Project, index: usize) -> Result<&Slide, CommandError> {
    p.slides.get(index).ok_or(CommandError::NoSlide(index))
}

fn slide_mut(p: &mut Project, index: usize) -> Result<&mut Slide, CommandError> {
    p.slides.get_mut(index).ok_or(CommandError::NoSlide(index))
}

fn element(p: &Project, s: usize, index: usize) -> Result<&Element, CommandError> {
    slide(p, s)?.elements.get(index).ok_or(CommandError::NoElement { slide: s, index })
}

fn element_mut(p: &mut Project, s: usize, index: usize) -> Result<&mut Element, CommandError> {
    slide_mut(p, s)?.elements.get_mut(index).ok_or(CommandError::NoElement { slide: s, index })
}
