//! as-vim — a minimal, vim-inspired terminal text editor.
//!
//! The crate is split into three modules:
//! - [`editor`]: all editor state and input handling (pure logic, no I/O side
//!   effects other than explicit save calls — fully unit-testable)
//! - [`input`]: maps crossterm events onto the editor's [`editor::Input`] enum
//! - [`ui`]: renders the [`editor::Editor`] state to the terminal

pub mod editor;
pub mod input;
pub mod syntax;
pub mod ui;
