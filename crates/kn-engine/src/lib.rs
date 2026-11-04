//! The editing engine: commands, undo history and saving. No UI types here,
//! so the CLI and automation can drive exactly what the editor drives.

mod command;
mod session;
pub mod storage;

pub use command::{Command, CommandError};
pub use session::Session;
