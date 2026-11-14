//! The tree each shared control gives assistive technology.
use super::tree::Harness;
use crate::i18n::t;
use crate::ui::tooltip::tip;
use gpui::accesskit::{Action, Role, Toggled};
use gpui::{ParentElement as _, Styled as _, div};
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn an_icon_button_is_named_by_its_tooltip_and_pressed_by_assistive_technology() {
    let pressed = Rc::new(Cell::new(0));
    let count = pressed.clone();
    let mut harness = Harness::new(move |_, _| {
        let count = count.clone();
        div()
            .flex()
            .child(
                crate::ui::icon_button::icon_button("filter", "toolbar-minus")
                    .tip(tip(t("Filter library")))
                    .on_click(move |_, _, _| count.set(count.get() + 1)),
            )
            .child(
                crate::ui::icon_button::icon_button("lock", "toolbar-plus")
                    .tip(tip(t("Lock filters")))
                    .pressed(true)
                    .disabled(true),
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
    let id = filter.id;
    harness.act(id, Action::Click);
    assert_eq!(pressed.get(), 1);
    assert!(harness.unexposed().is_empty(), "{:?}", harness.unexposed());
}
