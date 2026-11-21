//! The keys of a text field on each platform.
use super::editing::Platform;
use super::*;

/// The keys of a text field: AppKit's standard key bindings on a Mac (as
/// Chromium performs them in production's fields), the Windows and Linux
/// ones elsewhere. Undo and Redo come from the shortcut registry
/// (`keymap.rs`), which follows the user's keys for them.
pub fn bind(cx: &mut App) {
    cx.bind_keys(bindings(Platform::CURRENT));
}

/// The keys of a text field on `platform`.
fn bindings(platform: Platform) -> Vec<KeyBinding> {
    let field = Some("TextInput");
    let area = Some("TextArea");
    let mut bindings = vec![
        KeyBinding::new("enter", Commit, field),
        KeyBinding::new("shift-enter", Newline, area),
        KeyBinding::new("up", Up, area),
        KeyBinding::new("down", Down, area),
        KeyBinding::new("shift-up", SelectUp, area),
        KeyBinding::new("shift-down", SelectDown, area),
        KeyBinding::new("escape", Cancel, field),
        KeyBinding::new("backspace", Backspace, field),
        KeyBinding::new("delete", Delete, field),
        KeyBinding::new("left", Left, field),
        KeyBinding::new("right", Right, field),
        KeyBinding::new("shift-left", SelectLeft, field),
        KeyBinding::new("shift-right", SelectRight, field),
    ];
    if platform == Platform::Mac {
        bindings.extend([
            KeyBinding::new("cmd-a", SelectAll, field),
            KeyBinding::new("cmd-v", Paste, field),
            KeyBinding::new("cmd-c", Copy, field),
            KeyBinding::new("cmd-x", Cut, field),
            KeyBinding::new("home", Home, field),
            KeyBinding::new("end", End, field),
            KeyBinding::new("shift-home", SelectToStart, field),
            KeyBinding::new("shift-end", SelectToEnd, field),
            KeyBinding::new("cmd-up", Home, field),
            KeyBinding::new("cmd-down", End, field),
            KeyBinding::new("cmd-shift-up", SelectToStart, field),
            KeyBinding::new("cmd-shift-down", SelectToEnd, field),
            KeyBinding::new("alt-left", WordLeft, field),
            KeyBinding::new("alt-right", WordRight, field),
            KeyBinding::new("alt-shift-left", SelectWordLeft, field),
            KeyBinding::new("alt-shift-right", SelectWordRight, field),
            KeyBinding::new("ctrl-alt-b", WordLeft, field),
            KeyBinding::new("ctrl-alt-f", WordRight, field),
            KeyBinding::new("ctrl-alt-shift-b", SelectWordLeft, field),
            KeyBinding::new("ctrl-alt-shift-f", SelectWordRight, field),
            KeyBinding::new("cmd-left", LineStart, field),
            KeyBinding::new("cmd-right", LineEnd, field),
            KeyBinding::new("cmd-shift-left", SelectLineStart, field),
            KeyBinding::new("cmd-shift-right", SelectLineEnd, field),
            KeyBinding::new("ctrl-left", LineStart, field),
            KeyBinding::new("ctrl-right", LineEnd, field),
            KeyBinding::new("ctrl-shift-left", SelectLineStart, field),
            KeyBinding::new("ctrl-shift-right", SelectLineEnd, field),
            KeyBinding::new("alt-up", ParagraphBackward, field),
            KeyBinding::new("alt-down", ParagraphForward, field),
            KeyBinding::new("alt-shift-up", SelectParagraphBackward, field),
            KeyBinding::new("alt-shift-down", SelectParagraphForward, field),
            // The Emacs keys every Mac text field takes.
            KeyBinding::new("ctrl-a", ParagraphStart, field),
            KeyBinding::new("ctrl-e", ParagraphEnd, field),
            KeyBinding::new("ctrl-shift-a", SelectParagraphStart, field),
            KeyBinding::new("ctrl-shift-e", SelectParagraphEnd, field),
            KeyBinding::new("ctrl-b", Left, field),
            KeyBinding::new("ctrl-f", Right, field),
            KeyBinding::new("ctrl-shift-b", SelectLeft, field),
            KeyBinding::new("ctrl-shift-f", SelectRight, field),
            KeyBinding::new("ctrl-p", Up, field),
            KeyBinding::new("ctrl-n", Down, field),
            KeyBinding::new("ctrl-shift-p", SelectUp, field),
            KeyBinding::new("ctrl-shift-n", SelectDown, field),
            KeyBinding::new("ctrl-h", Backspace, field),
            KeyBinding::new("ctrl-d", Delete, field),
            KeyBinding::new("ctrl-k", DeleteToParagraphEnd, field),
            KeyBinding::new("ctrl-y", Yank, field),
            KeyBinding::new("ctrl-t", Transpose, field),
            KeyBinding::new("ctrl-o", OpenLine, field),
            KeyBinding::new("alt-backspace", DeleteWordBackward, field),
            KeyBinding::new("ctrl-alt-backspace", DeleteWordBackward, field),
            KeyBinding::new("alt-delete", DeleteWordForward, field),
            KeyBinding::new("cmd-backspace", DeleteToLineStart, field),
            KeyBinding::new("ctrl-backspace", DeleteBackwardDecomposing, field),
            KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, field),
        ]);
    } else {
        bindings.extend([
            KeyBinding::new("ctrl-a", SelectAll, field),
            KeyBinding::new("ctrl-v", Paste, field),
            KeyBinding::new("shift-insert", Paste, field),
            KeyBinding::new("ctrl-c", Copy, field),
            KeyBinding::new("ctrl-insert", Copy, field),
            KeyBinding::new("ctrl-x", Cut, field),
            KeyBinding::new("shift-delete", Cut, field),
            KeyBinding::new("home", LineStart, field),
            KeyBinding::new("end", LineEnd, field),
            KeyBinding::new("shift-home", SelectLineStart, field),
            KeyBinding::new("shift-end", SelectLineEnd, field),
            KeyBinding::new("ctrl-home", Home, field),
            KeyBinding::new("ctrl-end", End, field),
            KeyBinding::new("ctrl-shift-home", SelectToStart, field),
            KeyBinding::new("ctrl-shift-end", SelectToEnd, field),
            KeyBinding::new("ctrl-left", WordLeft, field),
            KeyBinding::new("ctrl-right", WordRight, field),
            KeyBinding::new("ctrl-shift-left", SelectWordLeft, field),
            KeyBinding::new("ctrl-shift-right", SelectWordRight, field),
            KeyBinding::new("ctrl-backspace", DeleteWordBackward, field),
            KeyBinding::new("ctrl-delete", DeleteWordForward, field),
        ]);
    }
    if platform == Platform::Windows {
        // Windows' second Redo, as its own fields and Electron's Edit menu
        // take it there.
        bindings.push(KeyBinding::new("ctrl-y", Redo, field));
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::{Platform, bindings};
    use gpui::Keystroke;
    use std::collections::BTreeSet;

    const ALL: [Platform; 3] = [Platform::Mac, Platform::Windows, Platform::Linux];

    /// What `keys` does in a field on `platform`, by the action's name.
    fn action(platform: Platform, keys: &'static str) -> Option<&'static str> {
        let typed = Keystroke::parse(keys).unwrap();
        bindings(platform)
            .iter()
            .find(|binding| binding.match_keystrokes(&[typed.clone()]) == Some(false))
            .map(|binding| binding.action().name().trim_start_matches("text_input::"))
    }

    fn actions(platform: Platform) -> BTreeSet<&'static str> {
        bindings(platform)
            .iter()
            .map(|binding| binding.action().name().trim_start_matches("text_input::"))
            .collect()
    }

    #[test]
    fn the_clipboard_and_select_all_take_command_on_a_mac_and_control_elsewhere() {
        for (key, name) in [
            ("a", "SelectAll"),
            ("c", "Copy"),
            ("x", "Cut"),
            ("v", "Paste"),
        ] {
            let command: &'static str = format!("cmd-{key}").leak();
            let control: &'static str = format!("ctrl-{key}").leak();
            assert_eq!(action(Platform::Mac, command), Some(name));
            for platform in [Platform::Windows, Platform::Linux] {
                assert_eq!(action(platform, control), Some(name), "{platform:?}");
            }
        }
        // On a Mac, Control-A is the Emacs key for the paragraph's start.
        assert_eq!(action(Platform::Mac, "ctrl-a"), Some("ParagraphStart"));
    }

    #[test]
    fn words_move_with_option_on_a_mac_and_control_elsewhere() {
        let moves = [
            ("left", "WordLeft"),
            ("right", "WordRight"),
            ("shift-left", "SelectWordLeft"),
            ("shift-right", "SelectWordRight"),
            ("backspace", "DeleteWordBackward"),
            ("delete", "DeleteWordForward"),
        ];
        for (key, name) in moves {
            let option: &'static str = format!("alt-{key}").leak();
            let control: &'static str = format!("ctrl-{key}").leak();
            assert_eq!(action(Platform::Mac, option), Some(name));
            for platform in [Platform::Windows, Platform::Linux] {
                assert_eq!(action(platform, control), Some(name), "{platform:?}");
            }
        }
        // On a Mac, Control-Left goes to the row's start.
        assert_eq!(action(Platform::Mac, "ctrl-left"), Some("LineStart"));
    }

    #[test]
    fn home_and_end_go_by_row_off_a_mac_and_to_the_text_ends_on_one() {
        for platform in [Platform::Windows, Platform::Linux] {
            assert_eq!(action(platform, "home"), Some("LineStart"));
            assert_eq!(action(platform, "end"), Some("LineEnd"));
            assert_eq!(action(platform, "shift-end"), Some("SelectLineEnd"));
            assert_eq!(action(platform, "ctrl-home"), Some("Home"));
            assert_eq!(action(platform, "ctrl-shift-end"), Some("SelectToEnd"));
        }
        assert_eq!(action(Platform::Mac, "home"), Some("Home"));
        assert_eq!(action(Platform::Mac, "cmd-left"), Some("LineStart"));
        assert_eq!(action(Platform::Mac, "cmd-down"), Some("End"));
    }

    #[test]
    fn no_key_off_a_mac_needs_the_command_key() {
        for platform in [Platform::Windows, Platform::Linux] {
            for binding in bindings(platform) {
                let keystroke = binding.keystrokes()[0].inner();
                assert!(
                    !keystroke.modifiers.platform,
                    "{platform:?}: {} takes the command key",
                    binding.action().name()
                );
            }
        }
    }

    #[test]
    fn only_the_mac_conventions_are_missing_elsewhere() {
        let mac_only: BTreeSet<&str> = actions(Platform::Mac)
            .difference(&actions(Platform::Windows))
            .copied()
            .collect();
        // The Emacs keys every Mac field takes, Option-Up and -Down by
        // paragraph, Command-Delete and Control-Delete, and the
        // character palette: none has a counterpart in a Windows or Linux
        // field.
        let expected = BTreeSet::from([
            "DeleteBackwardDecomposing",
            "DeleteToLineStart",
            "DeleteToParagraphEnd",
            "OpenLine",
            "ParagraphBackward",
            "ParagraphEnd",
            "ParagraphForward",
            "ParagraphStart",
            "SelectParagraphBackward",
            "SelectParagraphEnd",
            "SelectParagraphForward",
            "SelectParagraphStart",
            "ShowCharacterPalette",
            "Transpose",
            "Yank",
        ]);
        assert_eq!(mac_only, expected);
        assert_eq!(
            actions(Platform::Windows),
            &actions(Platform::Linux) | &BTreeSet::from(["Redo"])
        );
    }

    #[test]
    fn no_key_does_two_things() {
        for platform in ALL {
            let mut seen = BTreeSet::new();
            for binding in bindings(platform) {
                let keys = binding.keystrokes()[0]
                    .inner()
                    .unparse(gpui::OperatingSystem::Mac);
                let context = binding.predicate().map(|context| context.to_string());
                assert!(
                    seen.insert((keys.clone(), context)),
                    "{platform:?}: {keys} is bound twice"
                );
            }
        }
    }
}
