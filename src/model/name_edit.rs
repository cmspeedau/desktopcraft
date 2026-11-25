use super::*;

/// Characters Windows doesn't allow in a file name.
pub const BAD_NAME_CHARS: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
/// What's selected in an in-place rename, the way Explorer does it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NameSelection {
    None,
    Stem,
    All,
}
/// Editing an icon's name in place, like Explorer's own rename: the text is
/// what the icon shows; an extension Windows hides (".lnk", or every extension
/// when "File name extensions" is off) is kept unseen and put back on save. A
/// visible extension stays put while the name before it starts selected.
#[derive(Clone, Debug)]
pub struct NameEdit {
    pub text: String,
    pub selection: NameSelection,
    /// Caret position in characters (used when nothing is selected).
    pub caret: usize,
    /// The extension Windows doesn't show for this file (".lnk"), or "".
    pub hidden_ext: String,
    original: String,
}
impl NameEdit {
    pub fn new(shown: &str, file_name: &str, folder: bool) -> Self {
        let hidden_ext = if !folder
            && file_name.len() > shown.len()
            && file_name.starts_with(shown)
            && file_name[shown.len()..].starts_with('.')
        {
            file_name[shown.len()..].to_string()
        } else {
            String::new()
        };
        // Explorer selects the name up to its last dot, unless it's a folder,
        // the extension is hidden, or the name starts with the dot.
        let selection = match shown.rfind('.') {
            Some(i) if i > 0 && !folder && hidden_ext.is_empty() => NameSelection::Stem,
            _ => NameSelection::All,
        };
        Self {
            text: shown.to_string(),
            selection,
            caret: shown.chars().count(),
            hidden_ext,
            original: shown.to_string(),
        }
    }
    fn len(&self) -> usize {
        self.text.chars().count()
    }
    /// How many characters at the start are selected.
    pub fn selected_chars(&self) -> usize {
        match self.selection {
            NameSelection::None => 0,
            NameSelection::All => self.len(),
            NameSelection::Stem => self
                .text
                .rfind('.')
                .map_or(0, |i| self.text[..i].chars().count()),
        }
    }
    /// Replace the selection; the caret ends after what was put in.
    fn replace_selection(&mut self, with: &str) {
        let n = self.selected_chars();
        let rest: String = self.text.chars().skip(n).collect();
        self.text = format!("{with}{rest}");
        self.caret = with.chars().count();
        self.selection = NameSelection::None;
    }
    fn byte_at(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map_or(self.text.len(), |(i, _)| i)
    }
    /// Type text; returns false when it held characters a file name can't.
    pub fn type_text(&mut self, typed: &str) -> bool {
        let clean: String = typed
            .chars()
            .filter(|c| !BAD_NAME_CHARS.contains(c) && !c.is_control())
            .collect();
        if !clean.is_empty() {
            if self.selection == NameSelection::None {
                let at = self.byte_at(self.caret.min(self.len()));
                self.text.insert_str(at, &clean);
                self.caret = self.caret.min(self.len()) + clean.chars().count();
            } else {
                self.replace_selection(&clean);
            }
        }
        clean.chars().count() == typed.chars().filter(|c| !c.is_control()).count()
    }
    pub fn backspace(&mut self) {
        if self.selection != NameSelection::None {
            self.replace_selection("");
        } else if self.caret > 0 {
            let at = self.byte_at(self.caret - 1);
            self.text.remove(at);
            self.caret -= 1;
        }
    }
    pub fn delete(&mut self) {
        if self.selection != NameSelection::None {
            self.replace_selection("");
        } else if self.caret < self.len() {
            let at = self.byte_at(self.caret);
            self.text.remove(at);
        }
    }
    /// Left/Right like a text box: with a selection they jump to its start or end.
    pub fn left(&mut self) {
        if self.selection != NameSelection::None {
            self.caret = 0;
        } else {
            self.caret = self.caret.saturating_sub(1);
        }
        self.selection = NameSelection::None;
    }
    pub fn right(&mut self) {
        if self.selection != NameSelection::None {
            self.caret = self.selected_chars();
        } else {
            self.caret = (self.caret + 1).min(self.len());
        }
        self.selection = NameSelection::None;
    }
    pub fn home(&mut self) {
        self.caret = 0;
        self.selection = NameSelection::None;
    }
    pub fn end(&mut self) {
        self.caret = self.len();
        self.selection = NameSelection::None;
    }
    pub fn select_all(&mut self) {
        self.selection = NameSelection::All;
    }
    /// The new file name to rename to, or None when nothing would change.
    /// Like Windows, trailing spaces and dots are dropped.
    pub fn new_file_name(&self) -> Option<String> {
        let shown = self.text.trim().trim_end_matches(['.', ' ']);
        (!shown.is_empty() && shown != self.original).then(|| format!("{shown}{}", self.hidden_ext))
    }
    /// The text the icon should show after the rename.
    pub fn shown(&self) -> &str {
        self.text.trim().trim_end_matches(['.', ' '])
    }
}
