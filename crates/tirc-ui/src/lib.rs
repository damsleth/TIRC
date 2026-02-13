//! Ratatui rendering and terminal input handling.

pub mod engine;
pub mod input;
pub mod palette;

pub use engine::{UiEngine, UiIntent};
pub use input::{InputEditor, InputHistory};
pub use palette::PaletteEngine;
