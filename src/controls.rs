use crate::*;

mod rename;
mod folder_view;
mod shortcuts;
mod file_actions;
mod context_menu;
mod pointer;
mod pointer_press;
mod pointer_scroll;
mod drag_update;
mod drag_release;

use file_actions::FileAction;

#[cfg(test)]
mod docking_tests;
#[cfg(test)]
mod windows_file_ux_tests;
