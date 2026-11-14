//! The tree each shared control gives assistive technology: its role, name,
//! value, states and actions, and that the actions do what the pointer
//! does. Each test also checks that nothing it drew takes a click without a
//! role (see `guard_every_clickable_has_a_role_and_every_control_a_name`).
use super::tree::{Harness, Tree};
use crate::i18n::t;
use crate::ui::tooltip::tip;
use crate::ui::{self, icon_button::icon_button};
use gpui::accesskit::{Action, Role, Toggled};
use gpui::{
    AppContext as _, Focusable as _, IntoElement, ParentElement as _, Styled as _, div, point, px,
};
use std::cell::Cell;
use std::rc::Rc;

/// Nodes whose role is a control assistive technology presses or edits.
const CONTROLS: [Role; 16] = [
    Role::Button,
    Role::CheckBox,
    Role::RadioButton,
    Role::Switch,
    Role::Tab,
    Role::ComboBox,
    Role::ListBoxOption,
    Role::Slider,
    Role::SpinButton,
    Role::MenuItem,
    Role::MenuItemCheckBox,
    Role::MenuItemRadio,
    Role::TreeItem,
    Role::TextInput,
    Role::SearchInput,
    Role::MultilineTextInput,
];

/// What every test asserts of a whole tree: each control has a name (a
/// field may be known by its placeholder instead), and the frame drew no
/// clickable element without a role.
pub fn assert_complete(harness: &mut Harness, tree: &Tree) {
    for node in tree.all() {
        if CONTROLS.contains(&node.role()) {
            let named = !node.name().is_empty() || node.node().placeholder().is_some();
            assert!(
                named,
                "an unnamed {:?} in\n{}",
                node.role(),
                tree.dump()
            );
        }
    }
    let unexposed = harness.unexposed();
    assert!(
        unexposed.is_empty(),
        "clickable elements with no role: {unexposed:?}\n{}",
        tree.dump()
    );
}

#[test]
fn an_icon_button_is_named_by_its_tooltip_and_pressed_by_assistive_technology() {
    let pressed = Rc::new(Cell::new(0));
    let count = pressed.clone();
    let mut harness = Harness::new(move |_, _| {
        let count = count.clone();
        div()
            .flex()
            .child(
                icon_button("filter", "toolbar-minus")
                    .tip(tip(t("Filter library")))
                    .on_click(move |_, _, _| count.set(count.get() + 1)),
            )
            .child(
                icon_button("lock", "toolbar-plus")
                    .tip(tip(t("Lock filters")))
                    .pressed(true)
                    .disabled(true),
            )
            .child(
                icon_button("close", "window-close")
                    .tip(tip(t("Close")))
                    .name(t("Close Quick Look")),
            )
    });
    let tree = harness.tree();
    let filter = tree.find(Role::Button, "Filter library");
    assert!(filter.supports(Action::Click));
    assert!(filter.supports(Action::Focus));
    let lock = tree.find(Role::Button, "Lock filters");
    assert_eq!(lock.toggled(), Some(Toggled::True));
    assert!(lock.disabled());
    assert!(!lock.supports(Action::Click));
    // A name given over the tooltip's text.
    tree.find(Role::Button, "Close Quick Look");
    let id = filter.id;
    assert_complete(&mut harness, &tree);
    harness.act(id, Action::Click);
    assert_eq!(pressed.get(), 1);
}

#[test]
fn focusing_a_control_from_assistive_technology_puts_the_keyboard_on_it() {
    let mut harness = Harness::new(|_, _| {
        div().child(
            ui::action_button("save", ui::action_button::Look::Primary)
                .label(t("Save"))
                .on_click(|_, _, _| {}),
        )
    });
    let tree = harness.tree();
    let save = tree.find(Role::Button, "Save").id;
    harness.act(save, Action::Focus);
    let tree = harness.tree();
    assert_eq!(tree.focused().id, save, "{}", tree.dump());
}

#[test]
fn buttons_checks_and_switches_report_their_state() {
    let mut harness = Harness::new(|_, _| {
        div()
            .flex()
            .flex_col()
            .child(
                ui::action_button("cancel", ui::action_button::Look::Standard)
                    .label(t("Cancel"))
                    .disabled(true),
            )
            .child(ui::check("auto", t("Show tooltips"), true).on_toggle(|_, _| {}))
            .child(
                ui::check("dark", t("Dark"), false)
                    .radio()
                    .on_toggle(|_, _| {}),
            )
            .child(
                ui::toggle("auto-tag", false)
                    .name(t("Auto-tag new imports"))
                    .on_toggle(|_, _| {}),
            )
            .child(ui::action_row("add", "plus", t("Add Tags")).on_click(|_, _, _| {}))
    });
    let tree = harness.tree();
    assert!(tree.find(Role::Button, "Cancel").disabled());
    let check = tree.find(Role::CheckBox, "Show tooltips");
    assert_eq!(check.toggled(), Some(Toggled::True));
    assert!(check.supports(Action::Click));
    assert_eq!(
        tree.find(Role::RadioButton, "Dark").toggled(),
        Some(Toggled::False)
    );
    assert_eq!(
        tree.find(Role::Switch, "Auto-tag new imports").toggled(),
        Some(Toggled::False)
    );
    tree.find(Role::Button, "Add Tags");
    assert_complete(&mut harness, &tree);
}

#[test]
fn tabs_are_a_tab_list_and_glyph_pills_a_group_of_pressed_buttons() {
    use ui::segmented::{Look, segment, segmented, tab};
    let mut harness = Harness::new(|_, _| {
        div()
            .flex()
            .child(
                segmented(
                    "mode",
                    Look::Tabs,
                    vec![tab(t("Find & Replace")), tab(t("Sequence"))],
                    Some(1),
                )
                .name(t("Rename mode"))
                .on_select(|_, _, _| {}),
            )
            .child(
                segmented(
                    "logic",
                    Look::Logic,
                    vec![
                        segment("filter-any", t("Match any")),
                        segment("filter-all", t("Match all")),
                    ],
                    Some(0),
                )
                .name(t("Filter matching rule"))
                .on_select(|_, _, _| {}),
            )
    });
    let tree = harness.tree();
    let list = tree.find(Role::TabList, "Rename mode");
    let sequence = list.find(Role::Tab, "Sequence");
    assert_eq!(sequence.selected(), Some(true));
    assert_eq!(
        list.find(Role::Tab, "Find & Replace").selected(),
        Some(false)
    );
    let group = tree.find(Role::Group, "Filter matching rule");
    assert_eq!(
        group.find(Role::Button, "Match any").toggled(),
        Some(Toggled::True)
    );
    assert_complete(&mut harness, &tree);
}

#[test]
fn a_select_is_a_combo_box_showing_its_choice() {
    use ui::select::{choice, select};
    let mut harness = Harness::new(|_, _| {
        div().child(
            select(
                "language",
                "de",
                vec![choice("en", "English"), choice("de", "Deutsch")],
            )
            .name(t("Language"))
            .on_change(|_, _, _| {}),
        )
    });
    let tree = harness.tree();
    let combo = tree.find(Role::ComboBox, "Language");
    assert_eq!(combo.value(), Some("Deutsch"));
    assert_eq!(combo.expanded(), Some(false));
    assert!(combo.supports(Action::Click));
    assert_complete(&mut harness, &tree);
}

#[test]
fn a_slider_is_stepped_and_set_by_assistive_technology() {
    let value = Rc::new(Cell::new(200.));
    let shown = value.clone();
    let mut harness = Harness::new(move |_, _| {
        let set = shown.clone();
        div().child(
            ui::slider::slider("size", shown.get(), 100., 600., 50.)
                .name(t("Thumbnail size"))
                .on_change(move |next, _| set.set(next)),
        )
    });
    let tree = harness.tree();
    let slider = tree.find(Role::Slider, "Thumbnail size");
    assert_eq!(slider.node().numeric_value(), Some(200.));
    assert_eq!(slider.node().min_numeric_value(), Some(100.));
    assert_eq!(slider.node().max_numeric_value(), Some(600.));
    let id = slider.id;
    assert_complete(&mut harness, &tree);
    harness.act(id, Action::Increment);
    assert_eq!(value.get(), 250.);
    let tree = harness.tree();
    let id = tree.find(Role::Slider, "Thumbnail size").id;
    harness.act(id, Action::Decrement);
    assert_eq!(value.get(), 200.);
}

#[test]
fn a_text_field_exposes_its_text_placeholder_and_selection() {
    use ui::text_input::{Kind, TextInput};
    let mut harness = Harness::new(|window, cx| {
        let input = window.use_keyed_state("input", cx, |_, cx| {
            let mut input = TextInput::new(Kind::Form, cx)
                .placeholder("Folder name")
                .name(t("Folder name"));
            input.set_text("Holidays".to_owned(), cx);
            input.caret_at_end(cx);
            input
        });
        div().w(px(200.)).child(input)
    });
    let tree = harness.tree();
    let field = tree.find(Role::TextInput, "Folder name");
    assert_eq!(field.value(), Some("Holidays"));
    assert_eq!(field.node().placeholder(), Some("Folder name"));
    let run = field.children()[0];
    assert_eq!(run.role(), Role::TextRun);
    assert_eq!(run.value(), Some("Holidays"));
    let selection = field.node().text_selection().expect("a caret");
    assert_eq!(selection.focus.node, run.id);
    assert_eq!(
        selection.focus.character_index, 8,
        "the caret after the text"
    );
    // Focusing the field from assistive technology puts the keyboard there.
    let id = field.id;
    assert!(field.supports(Action::Focus));
    assert_complete(&mut harness, &tree);
    harness.act(id, Action::Focus);
    assert_eq!(harness.tree().focused().id, id);
}

#[test]
fn a_number_field_is_a_spin_button_between_its_steppers() {
    let mut harness = Harness::new(|_, _| {
        div().child(
            ui::number_input::number_input("posts", 50, 1, 500)
                .name(t("Posts per run"))
                .on_commit(|_, _| {}),
        )
    });
    let tree = harness.tree();
    let spin = tree.find(Role::SpinButton, "Posts per run");
    assert_eq!(spin.node().numeric_value(), Some(50.));
    tree.find(Role::Button, "Increase posts per run");
    tree.find(Role::Button, "Decrease posts per run");
    assert_complete(&mut harness, &tree);
}

#[test]
fn a_dialog_is_named_by_its_title_and_holds_the_focus() {
    let mut harness = Harness::new(|window, cx| {
        let focus = window
            .use_keyed_state("focus", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let presence = window
            .use_keyed_state("presence", cx, |_, _| ui::modal::Presence::new())
            .read(cx)
            .clone();
        if !focus.is_focused(window) {
            window.focus(&focus, cx);
        }
        div().size_full().child(
            ui::confirm::confirm(
                "delete",
                t("Delete tag"),
                t("This removes the tag from every item."),
                t("Delete"),
                presence,
            )
            .focus(&focus)
            .on_close(|_, _| {})
            .on_confirm(|_, _| {}),
        )
    });
    // The dialog fades in; the tree is there from the first frame.
    let tree = harness.tree();
    let dialog = tree.find(Role::Dialog, "Delete tag");
    assert!(dialog.node().is_modal());
    assert_eq!(tree.focused().id, dialog.id, "{}", tree.dump());
    dialog.find(Role::Button, "Close");
    dialog.find(Role::Button, "Cancel");
    dialog.find(Role::Button, "Delete");
    assert!(
        dialog
            .descendants()
            .iter()
            .any(|node| node.role() == Role::Label
                && node.value() == Some("This removes the tag from every item.")),
        "{}",
        tree.dump()
    );
    assert_complete(&mut harness, &tree);
}

#[test]
fn a_menu_is_a_menu_of_items_with_checks_shortcuts_and_submenus() {
    use ui::menu::{Menu, item, submenu};
    let mut harness = Harness::new(|window, cx| {
        let menu = window.use_keyed_state("menu", cx, |window, cx| {
            let menu = Menu::new(
                vec![
                    item("open", "Open").shortcut("⏎").into(),
                    item("pin", "Pin Inspector").checked(true).into(),
                    item("delete", "Move to Trash").disabled(true).into(),
                    submenu("Sort By", vec![item("name", "Name").into()]).into(),
                ],
                point(px(10.), px(10.)),
                cx,
            )
            .in_window();
            window.focus(&menu.focus_handle(cx), cx);
            menu
        });
        div().size_full().child(menu)
    });
    let tree = harness.tree();
    let menu = tree.find(Role::Menu, "Context menu");
    let open = menu.find(Role::MenuItem, "Open");
    assert_eq!(open.node().keyboard_shortcut(), Some("⏎"));
    assert!(open.supports(Action::Click));
    assert_eq!(
        menu.find(Role::MenuItemCheckBox, "Pin Inspector").toggled(),
        Some(Toggled::True)
    );
    assert!(menu.find(Role::MenuItem, "Move to Trash").disabled());
    assert_eq!(
        menu.find(Role::MenuItem, "Sort By").expanded(),
        Some(false)
    );
    assert_eq!(tree.focused().id, menu.id);
    assert_complete(&mut harness, &tree);
}

#[test]
fn a_notification_is_an_alert_or_status_in_a_live_region() {
    use gpui::accesskit::Live;
    let mut harness = Harness::new(|window, cx| {
        let host = window.use_keyed_state("host", cx, |_, cx| ui::notify::Host::new(cx));
        div().size_full().child(host)
    });
    let tree = harness.tree();
    let region = tree.with_role(Role::Group);
    assert!(
        region.iter().any(|node| node.node().live() == Some(Live::Polite)),
        "the live region is there before anything is announced\n{}",
        tree.dump()
    );
    harness
        .cx
        .update(|cx| ui::notify::error(t("Could not move folder"), "busy", cx));
    let tree = harness.tree();
    let alert = tree.find(Role::Alert, "Could not move folder — busy");
    alert.find(Role::Button, "Dismiss notification");
    assert!(
        tree.all()
            .iter()
            .any(|node| node.node().live() == Some(Live::Assertive))
    );
    assert_complete(&mut harness, &tree);
}

#[test]
fn sections_chips_progress_and_dates_describe_themselves() {
    let mut harness = Harness::new(|_, cx| {
        div()
            .flex()
            .flex_col()
            .child(
                ui::panel_section::panel_section("tags", t("Tags"))
                    .expanded(true)
                    .on_toggle(|_, _, _| {})
                    .content(Some(
                        ui::chip("tag", "blue sky", 0x00aaff)
                            .namespace(Some("general".into()))
                            .on_remove(|_, _, _| {})
                            .into_any_element(),
                    )),
            )
            .child(
                ui::section_header::section_header("folders", t("Folders"))
                    .count(1234)
                    .expanded(false)
                    .on_toggle(|_, _, _| {}),
            )
            .child(ui::progress_bar::progress_bar().filled("scan", 0.25))
            .child(
                ui::date_picker::date_picker("from", "2025-02-03")
                    .name(t("From"))
                    .on_change(|_, _, _| {}),
            )
            .child(ui::empty_state(
                "folder",
                t("All healthy"),
                t("No problems found."),
                Vec::new(),
                cx,
            ))
    });
    let tree = harness.tree();
    let group = tree.find(Role::Group, "Tags");
    let heading = group.find(Role::Heading, "Tags");
    assert_eq!(heading.node().level(), Some(2));
    assert_eq!(heading.expanded(), Some(true));
    let chip = group.find(Role::ListItem, "general:blue sky");
    chip.find(Role::Button, "Remove");
    let folders = tree.find(Role::Button, &format!("Folders ({})", crate::grid::format::grouped(1234u64)));
    assert_eq!(folders.expanded(), Some(false));
    let progress = tree.with_role(Role::ProgressIndicator);
    assert_eq!(progress[0].node().numeric_value(), Some(25.));
    let date = tree.find(Role::Button, "From");
    assert_eq!(date.expanded(), Some(false));
    assert!(date.value().is_some());
    tree.find(Role::Heading, "All healthy");
    assert_complete(&mut harness, &tree);
}

#[test]
fn a_sidebar_row_is_a_tree_item_at_its_level() {
    let mut harness = Harness::new(|_, _| {
        div().child(
            ui::sidebar_row::sidebar_row("row", "folder", "Holidays")
                .depth(1)
                .count(Some(12))
                .active(true)
                .disclosure(Some(false))
                .on_click(|_, _, _| {})
                .on_toggle(|_, _, _| {}),
        )
    });
    let tree = harness.tree();
    let row = tree.find(Role::TreeItem, "Holidays 12");
    assert_eq!(row.node().level(), Some(2));
    assert_eq!(row.selected(), Some(true));
    assert_eq!(row.expanded(), Some(false));
    row.find(Role::Button, "Expand Holidays 12");
    assert_complete(&mut harness, &tree);
}

#[test]
fn the_interface_zoom_scales_the_tree_to_the_window() {
    let mut harness = Harness::new(|_, _| {
        div().child(
            ui::action_button("save", ui::action_button::Look::Primary)
                .label(t("Save"))
                .on_click(|_, _, _| {}),
        )
    });
    harness.cx.update(|cx| cx.set_ui_zoom(1.5));
    let tree = harness.tree();
    let transform = tree.root().node().transform().expect("a zoomed root");
    assert_eq!(transform.as_coeffs()[0], 1.5);
}

/// Every `.rs` file under `src/`, with its path from there.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                out.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out
}

/// A glyph button shows no text, so assistive technology knows it only by
/// the name it is given: its tooltip (production's `aria-label` repeats
/// it), a label, or a name of its own. Every call that makes one gives it
/// one of these before the statement ends.
#[test]
fn guard_every_glyph_button_is_named() {
    // The builders that take no tooltip themselves.
    const BUILDERS: [&str; 3] = ["icon_button(", "small_button(", "filter_action("];
    let mut unnamed = Vec::new();
    for (file, text) in sources() {
        if file.starts_with("ui/a11y/") {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            let Some(builder) = BUILDERS.iter().find(|builder| code.contains(*builder)) else {
                continue;
            };
            // The definitions, calls that only forward to one, a view's own
            // method of the same name, and a call marked `// a11y: <why>`
            // on its line or the line above.
            let above = |back: usize| lines[at.saturating_sub(back)..at].join("\n");
            if code.contains(&format!("fn {builder}"))
                || code.contains(&format!("self.{builder}"))
                || line.contains("// a11y:")
                || above(1).contains("// a11y:")
                || above(3).contains("IconButton {")
                || code.contains(&format!("pub use {}", builder.trim_end_matches('(')))
            {
                continue;
            }
            // The chain, up to the end of its statement or expression.
            let depth = line.len() - line.trim_start().len();
            let mut chain = String::new();
            for next in lines.iter().skip(at).take(40) {
                chain.push_str(next);
                let indent = next.len() - next.trim_start().len();
                if next.trim_end().ends_with(';')
                    || (indent < depth && !next.trim().is_empty() && chain.len() > line.len())
                {
                    break;
                }
            }
            let named = [".tip(", ".name(", ".label("]
                .iter()
                .any(|call| chain.contains(call))
                // A wrapper that returns the button for its callers to name.
                || chain.contains("-> ui::icon_button::IconButton")
                || chain.contains("-> super::icon_button::IconButton")
                || chain.contains("-> IconButton");
            if !named {
                unnamed.push(format!("{file}:{}", at + 1));
            }
        }
    }
    assert!(
        unnamed.is_empty(),
        "glyph buttons with no tooltip, label or name:\n  {}",
        unnamed.join("\n  ")
    );
}

/// Each shared control that takes a click or the keyboard says what it is:
/// a file of `ui/` that makes an element clickable or a tab stop sets a role
/// there (or declares the click presentational).
#[test]
fn guard_every_interactive_shared_control_has_a_role() {
    let mut missing = Vec::new();
    for (file, text) in sources() {
        if !file.starts_with("ui/") || file.starts_with("ui/a11y") || file == "ui/focus.rs" {
            continue;
        }
        let interactive = text.contains(".on_click(") || text.contains("focus::control(");
        let described = text.contains(".role(")
            || text.contains("// a11y:")
            || text.contains("a11y_presentational")
            || text.contains("a11y::text(")
            || text.contains("a11y::heading(");
        if interactive && !described {
            missing.push(file);
        }
    }
    assert!(
        missing.is_empty(),
        "shared controls that take input but set no role: {missing:?}"
    );
}
