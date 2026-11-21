//! A field that shows the chosen tags as chips, followed by a search field for
//! adding more. 120 ms after the user stops typing, the library's tags are
//! searched, and up to eight tags that are not chosen yet are listed below the
//! field. The arrow keys move through the list. Enter or Tab adds the
//! highlighted tag. Enter adds the typed text when no list is shown. Backspace
//! in the empty search field removes the last tag.
use super::popover::{Placement, Side, floating, place};
use super::text_input::{EmptyBackspace, Submitted, TextInput};
use crate::i18n::t;
use crate::theme;
use crate::theme::Text as _;
use futures::channel::oneshot;
use gpui::{prelude::FluentBuilder as _, *};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

actions!(tag_token_input, [Next, Previous, Choose]);

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", Next, Some("TagTokenInput")),
        KeyBinding::new("up", Previous, Some("TagTokenInput")),
        KeyBinding::new("tab", Choose, Some("TagTokenInput")),
    ]);
}

/// A tag that the search found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub namespace: String,
    pub subname: String,
}

impl Found {
    /// Returns the tag's full name: `namespace:subname`, or the subname alone
    /// for a tag in General.
    pub fn name(&self) -> String {
        crate::tags::name(&self.namespace, &self.subname)
    }
}

/// The function that searches the library's tags for a query.
pub type Search = Rc<dyn Fn(String, &mut App) -> oneshot::Receiver<Vec<Found>>>;

/// The event that is emitted when the list of chosen tags has changed.
pub struct Changed;

const SEARCH_DELAY: Duration = Duration::from_millis(120);
const RESULTS: usize = 8;
/// The largest height of the list of found tags.
const RESULTS_HEIGHT: f32 = 200.;

pub struct TagTokenInput {
    values: Vec<String>,
    input: Entity<TextInput>,
    search: Search,
    query: String,
    results: Vec<Found>,
    open: bool,
    selected: usize,
    /// The task that runs the search after the user has stopped typing. The
    /// next keystroke replaces it.
    pending: Option<Task<()>>,
    /// The bounds of the field's surface. The list of found tags opens below
    /// them.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    compact: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Changed> for TagTokenInput {}

impl TagTokenInput {
    pub fn new(values: Vec<String>, search: Search, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(super::text_input::Kind::Form, cx)
                .placeholder("Search tags...")
                .submits()
        });
        let subscriptions = vec![
            cx.observe(&input, |this, input, cx| {
                let query = input.read(cx).value();
                if query != this.query {
                    this.query = query.clone();
                    this.search_for(query, cx);
                }
            }),
            cx.subscribe(&input, |this, _, _: &Submitted, cx| this.submit(cx)),
            cx.subscribe(&input, |this, _, _: &EmptyBackspace, cx| {
                if let Some(last) = this.values.pop() {
                    let _ = last;
                    cx.emit(Changed);
                    cx.notify();
                }
            }),
        ];
        Self {
            values,
            input,
            search,
            query: String::new(),
            results: Vec::new(),
            open: false,
            selected: 0,
            pending: None,
            bounds: Rc::new(Cell::new(Bounds::default())),
            compact: false,
            _subscriptions: subscriptions,
        }
    }

    /// Makes the field 32px high with less padding.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn values(&self) -> &[String] {
        &self.values
    }

    fn search_for(&mut self, query: String, cx: &mut Context<Self>) {
        if query.trim().is_empty() {
            self.pending = None;
            self.results.clear();
            self.open = false;
            cx.notify();
            return;
        }
        let search = self.search.clone();
        self.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let Ok(answer) = this.update(cx, |_, cx| search(query, cx)) else {
                return;
            };
            let found = answer.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                let next = found
                    .into_iter()
                    .filter(|tag| !this.values.contains(&tag.name()))
                    .take(RESULTS)
                    .collect::<Vec<_>>();
                this.open = !next.is_empty();
                this.results = next;
                this.selected = 0;
                cx.notify();
            });
        }));
    }

    /// Adds `tag` to the chosen tags unless it is empty or already chosen, and
    /// empties the search field.
    fn add(&mut self, tag: String, cx: &mut Context<Self>) {
        let tag = tag.trim().to_owned();
        if !tag.is_empty() && !self.values.contains(&tag) {
            self.values.push(tag);
            cx.emit(Changed);
        }
        self.pending = None;
        self.results.clear();
        self.open = false;
        self.query.clear();
        self.input.update(cx, |input, cx| input.clear(cx));
        cx.notify();
    }

    fn remove(&mut self, tag: &str, cx: &mut Context<Self>) {
        self.values.retain(|value| value != tag);
        cx.emit(Changed);
        cx.notify();
    }

    /// Handles Enter: adds the highlighted tag of the list, or the typed text
    /// when no list is shown.
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.open
            && let Some(found) = self.results.get(self.selected)
        {
            let name = found.name();
            self.add(name, cx);
        } else if !self.query.trim().is_empty() {
            let typed = self.query.clone();
            self.add(typed, cx);
        }
    }
}

impl Render for TagTokenInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let focused = self.input.focus_handle(cx).contains_focused(window, cx);
        // The search field shows its placeholder only while no tag is chosen.
        let placeholder = if self.values.is_empty() {
            t("Search tags...")
        } else {
            ""
        };
        self.input
            .update(cx, |input, _| input.set_placeholder(placeholder));
        let mut surface = div()
            .id("tag-token-surface")
            .role(Role::Group)
            .relative()
            .w_full()
            .min_h(px(if self.compact { 32. } else { 56. }))
            .px(px(6.))
            .py(px(if self.compact { 3. } else { 5. }))
            .flex()
            .flex_wrap()
            .items_center()
            .content_start()
            .gap(px(4.))
            .rounded(px(theme::RADIUS_SM))
            .border_1()
            .border_color(if focused {
                theme.primary_at(50.)
            } else {
                theme.border()
            })
            .bg(theme.control_surface())
            .cursor_text()
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.input.focus_handle(cx), cx);
            }));
        for (position, tag) in self.values.iter().enumerate() {
            // A tag without a namespace is in General.
            let (namespace, subtag) = crate::tags::split(tag);
            let namespace = namespace.unwrap_or("general");
            let removed = tag.clone();
            surface = surface.child(
                super::chip(
                    ("tag-token", position),
                    subtag.to_owned(),
                    crate::tags::chip_color(namespace),
                )
                .namespace(
                    crate::tags::group_of(namespace)
                        .filter(|_| crate::settings::current(cx).show_tag_prefixes)
                        .map(SharedString::from),
                )
                .on_remove(cx.listener(move |this, _, _, cx| this.remove(&removed, cx))),
            );
        }
        let bounds = self.bounds.clone();
        surface = surface
            .child(
                div()
                    .flex_1()
                    .min_w(px(90.))
                    .h(px(theme::CONTROL_SM))
                    .px(px(2.))
                    .text_color(theme.text())
                    .child(self.input.clone()),
            )
            .child(
                canvas(move |area, _, _| bounds.set(area), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            );
        let mut root = div()
            .relative()
            .w_full()
            .key_context("TagTokenInput")
            .on_action(cx.listener(|this, _: &Next, _, cx| {
                if this.open {
                    this.selected = (this.selected + 1).min(this.results.len().saturating_sub(1));
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &Previous, _, cx| {
                if this.open {
                    this.selected = this.selected.saturating_sub(1);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &Choose, _, cx| {
                if this.open {
                    this.submit(cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &super::text_input::Cancel, _, cx| {
                if this.open {
                    this.open = false;
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .child(surface);
        if self.open && focused {
            let area = self.bounds.get();
            let mut list = super::GlassPanel::new(
                div()
                    .id("tag-token-results")
                    .occlude()
                    .w(area.size.width)
                    .max_h(px(RESULTS_HEIGHT))
                    .overflow_y_scroll()
                    .p(px(4.))
                    .border_1()
                    .border_color(theme.border_secondary())
                    .bg(theme.glass()),
                theme::RADIUS_SM,
            )
            .backdrop_blur(theme.glass_blur())
            .drop_shadows(theme.panel_shadow())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_SM);
            for (index, found) in self.results.iter().enumerate() {
                let name = found.name();
                list = list.child(
                    div()
                        .id(("tag-token-result", index))
                        .w_full()
                        .min_h(px(28.))
                        .px(px(4.))
                        .py(px(2.))
                        .flex()
                        .items_center()
                        .rounded(px(theme::RADIUS_SM))
                        .cursor_pointer()
                        .when(index == self.selected, |row| row.bg(theme.surface_hover()))
                        .hover(move |style| style.bg(theme.surface_hover()))
                        // The tag is added on the press, because the field
                        // loses the keyboard focus before a click is complete.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.add(name.clone(), cx);
                            }),
                        )
                        .child(
                            super::chip(
                                ("tag-token-found", index),
                                found.subname.clone(),
                                crate::tags::chip_color(&found.namespace),
                            )
                            .namespace(
                                crate::tags::group_of(&found.namespace)
                                    .filter(|_| crate::settings::current(cx).show_tag_prefixes)
                                    .map(SharedString::from),
                            ),
                        ),
                );
            }
            // The list is as wide as the field and opens 2px below it, also
            // when it then reaches beyond the window. It is on the layer
            // `OVER_DIALOG`, so that in a dialog it is drawn above the dialog.
            let corner = place(
                area,
                size(f32::from(area.size.width), RESULTS_HEIGHT),
                window.viewport_size(),
                Placement::new(Side::Below, 2., 0.).unclamped(),
            );
            root = root.child(
                floating("tag-token-floating", super::layer::OVER_DIALOG)
                    .anchored(corner)
                    .child(list),
            );
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::Found;

    #[test]
    fn tags_are_written_as_the_earlier_versions_input_wrote_them() {
        let general = Found {
            namespace: String::new(),
            subname: "cat".into(),
        };
        assert_eq!(general.name(), "cat");
        let named = Found {
            namespace: "artist".into(),
            subname: "ann".into(),
        };
        assert_eq!(named.name(), "artist:ann");
    }
}
