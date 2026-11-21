//! The filter bar under the titlebar. It holds a control for each filter
//! field that is pinned or set, and each control opens the field's menu or
//! picker. After the controls comes the button that pins fields. On the
//! right are the saved filters, the lock that keeps the filters when the
//! user opens another destination, and the button that clears all filters.
use super::filter_editors::{
    self, DateRangeEditor, DatesCommitted, RangeCommitted, RangeEditor, ResolutionEditor,
    SaveFilterEditor, SaveRequested,
};
use super::filters::{self, Field, Filters, TagChoice};
use crate::i18n::t;
use crate::inspector::{
    FolderPicker, TagPicker, folder_picker::Event as FolderEvent, tag_picker::Event as TagEvent,
};
use crate::session::{Change, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    color_editor::{self, ColorCommitted, ColorEditor},
    filter_control::{Glyph, filter_action, filter_control},
    menu::{self, Menu},
};
use gpui::*;
use picto_library::predicate::ViewQuerySpec;
use picto_library::query::{ItemScope, RootQuery};

/// The smallest height of the filter bar, in pixels.
pub const HEIGHT: f32 = 38.;

/// Emitted when the bar opens a menu, so that the window draws the menu
/// above everything else.
pub enum BarEvent {
    Menu(Entity<Menu>),
}

impl EventEmitter<BarEvent> for FilterBar {}

/// The kinds of menu that the bar opens.
#[derive(Clone, Copy, PartialEq)]
enum MenuKind {
    Rating,
    Type,
    Pin,
    Imported,
    Modified,
    Color,
    Duration,
    Size,
    Resolution,
    Saved,
}

impl MenuKind {
    fn has_editor(self) -> bool {
        !matches!(self, MenuKind::Rating | MenuKind::Type | MenuKind::Pin)
    }

    /// Returns the menu's width in pixels, or `None` for a menu that keeps
    /// the default width of 270 px.
    fn width(self) -> Option<f32> {
        match self {
            MenuKind::Saved => Some(250.),
            MenuKind::Color => Some(230.),
            MenuKind::Imported | MenuKind::Modified => Some(310.),
            MenuKind::Duration | MenuKind::Size | MenuKind::Resolution => Some(210.),
            _ => None,
        }
    }
}

/// The editors of the open menu. They are kept so that the menu's rows can
/// be built again around them. The colour, date and save editors are updated
/// when the filters change outside the menu. The range and resolution
/// editors keep the values they were opened with.
#[derive(Default)]
struct Editors {
    color: Option<Entity<ColorEditor>>,
    dates: Option<Entity<DateRangeEditor>>,
    range: Option<Entity<RangeEditor>>,
    resolution: Option<Entity<ResolutionEditor>>,
    save: Option<Entity<SaveFilterEditor>>,
}

pub struct FilterBar {
    session: Entity<Session>,
    /// The fields whose controls show while the fields are not set. The
    /// order of this list is not used: the bar shows the controls in the
    /// order of `Field::ALL`.
    pinned: Vec<Field>,
    /// The saved filters, each with its name.
    saved: Vec<(String, Filters)>,
    /// The MIME types found among the items that the filters other than the
    /// type filter leave. The type menu offers them.
    facets: Vec<String>,
    /// The destination, the search text and the filters that `facets` was
    /// last requested for.
    facet_query: Option<(usize, String, Filters)>,
    facet_task: Option<Task<()>>,
    menu: Option<(MenuKind, Entity<Menu>)>,
    editors: Editors,
    /// The tag picker and the folder picker, used in their filter mode. The
    /// window draws them over everything else.
    tag_picker: Entity<TagPicker>,
    folder_picker: Entity<FolderPicker>,
    /// Whether the bar is open, whether the filters are locked, and the
    /// filters, as the bar last drew them. The bar is drawn again only when
    /// one of them changes.
    shown: Option<(bool, bool, Filters)>,
    scroll: ScrollHandle,
    _menu: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl FilterBar {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let tag_picker = cx.new(|cx| TagPicker::new(session.clone(), cx));
        let folder_picker = cx.new(|cx| FolderPicker::new(session.clone(), cx));
        let subscriptions = vec![
            cx.subscribe(&session, |this, _, change: &Change, cx| {
                if matches!(change, Change::Results | Change::Navigation) {
                    this.session_changed(cx);
                }
            }),
            cx.subscribe(&tag_picker, |this, _, event: &TagEvent, cx| {
                if let TagEvent::FilterChanged {
                    include,
                    exclude,
                    mode,
                } = event
                {
                    let choices = |tags: &[(picto_library::TagId, String)]| {
                        tags.iter()
                            .map(|(id, name)| TagChoice {
                                tag_id: id.0,
                                name: name.clone(),
                            })
                            .collect::<Vec<_>>()
                    };
                    let (include, exclude, mode) = (choices(include), choices(exclude), *mode);
                    this.update(cx, |filters| {
                        filters.include_tags = include;
                        filters.exclude_tags = exclude;
                        filters.tag_mode = mode;
                    });
                }
            }),
            cx.subscribe(&folder_picker, |this, _, event: &FolderEvent, cx| {
                if let FolderEvent::FilterChanged {
                    include,
                    exclude,
                    mode,
                } = event
                {
                    let ids = |folders: &[picto_library::FolderId]| {
                        folders.iter().map(|folder| folder.0).collect::<Vec<_>>()
                    };
                    let (include, exclude, mode) = (ids(include), ids(exclude), *mode);
                    this.update(cx, |filters| {
                        filters.include_folders = include;
                        filters.exclude_folders = exclude;
                        filters.folder_mode = mode;
                    });
                }
            }),
        ];
        // The pinned fields and the saved filters are read from the app's
        // stored state.
        let kept = crate::state::get(cx);
        let pinned = kept
            .pinned_filters
            .iter()
            .filter_map(|id| Field::from_id(id))
            .collect();
        let saved = kept
            .saved_filters
            .iter()
            .map(|(name, filters)| (name.clone(), Filters::from_json(filters)))
            .collect();
        Self {
            session,
            pinned,
            saved,
            facets: Vec::new(),
            facet_query: None,
            facet_task: None,
            menu: None,
            editors: Editors::default(),
            tag_picker,
            folder_picker,
            shown: None,
            scroll: ScrollHandle::new(),
            _menu: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Writes the pinned fields and the saved filters to the app's stored
    /// state.
    fn keep(&self, cx: &mut App) {
        let pinned = self
            .pinned
            .iter()
            .map(|field| field.id().to_owned())
            .collect();
        let saved = self
            .saved
            .iter()
            .map(|(name, filters)| (name.clone(), filters.to_json()))
            .collect();
        crate::state::update(cx, |state| {
            state.pinned_filters = pinned;
            state.saved_filters = saved;
        });
    }

    fn filters(&self, cx: &App) -> Filters {
        self.session.read(cx).filters.clone()
    }

    fn update(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Filters)) {
        let mut filters = self.filters(cx);
        change(&mut filters);
        self.session
            .update(cx, |session, cx| session.set_filters(filters, cx));
    }

    /// Called when the session's results or navigation change. Closes the
    /// open menu when the bar is closed. Draws the bar again if what it
    /// shows changed, and then updates the rows and the editors of the open
    /// menu. While the bar is open, requests the types for the type menu
    /// again if they may differ.
    fn session_changed(&mut self, cx: &mut Context<Self>) {
        let (open, locked, filters) = {
            let session = self.session.read(cx);
            (
                session.filter_bar_open,
                session.filters_locked,
                session.filters.clone(),
            )
        };
        if !open && let Some((_, menu)) = self.menu.take() {
            menu.update(cx, |menu, cx| menu.dismiss(cx));
        }
        let shown = Some((open, locked, filters.clone()));
        if shown != self.shown {
            self.shown = shown;
            self.refresh_menu(cx);
            cx.notify();
        }
        if open {
            self.fetch_facets(cx);
        }
    }

    /// Requests the MIME types found among the items of the displayed
    /// destination that match the search text and every filter except the
    /// type filter. Does nothing when the last request was the same.
    fn fetch_facets(&mut self, cx: &mut Context<Self>) {
        let (key, scope, filter) = {
            let session = self.session.read(cx);
            let Some(entry) = session.nav.get(session.displayed) else {
                return;
            };
            if entry.screen.is_some() {
                return;
            }
            let rest = session.filters.without_types();
            let key = (
                session.displayed,
                session.search_text().to_owned(),
                rest.clone(),
            );
            let scope = match entry.scope.clone() {
                ItemScope::FolderTree { folder_id } if !session.prefs.show_subfolders => {
                    ItemScope::Folder { folder_id }
                }
                scope => scope,
            };
            let filter = picto_library::predicate::FilterExpr::All(vec![
                crate::backend::text_filter(session.search_text()),
                rest.to_expr(),
            ]);
            (key, scope, filter)
        };
        if self.facet_query.as_ref() == Some(&key) {
            return;
        }
        self.facet_query = Some(key);
        let answer = self.session.read(cx).run(move |library| {
            library.mime_facets(&RootQuery {
                scope,
                view: ViewQuerySpec {
                    filter,
                    ..Default::default()
                },
            })
        });
        self.facet_task = Some(cx.spawn(async move |this, cx| {
            let facets = match answer.await {
                Ok(Ok(facets)) => facets.values.into_iter().map(|facet| facet.mime).collect(),
                _ => Vec::new(),
            };
            let _ = this.update(cx, |this, cx| {
                this.facets = facets;
                if matches!(this.menu, Some((MenuKind::Type, _))) {
                    this.refresh_menu(cx);
                }
            });
        }));
    }

    // The functions below open the bar's menus and pickers and apply what
    // the user chooses in them.

    fn open_menu(&mut self, kind: MenuKind, anchor: Bounds<Pixels>, cx: &mut Context<Self>) {
        if let Some((_, menu)) = self.menu.take() {
            menu.update(cx, |menu, cx| menu.dismiss(cx));
        }
        self.editors = Editors::default();
        self._menu.clear();
        let filters = self.filters(cx);
        self.create_editors(kind, &filters, cx);
        let entries = self.entries(kind, &filters);
        // The menu opens 4 px below the control, with its left edge at the
        // control's left edge.
        let position = point(anchor.left(), anchor.bottom() + px(4.));
        let menu = cx.new(|cx| {
            let menu = Menu::new(entries, position, cx)
                .search(matches!(kind, MenuKind::Type | MenuKind::Pin));
            // A menu with an editor is drawn inside the window, because a
            // text field needs the keyboard and a date field's calendar opens
            // over the window.
            let menu = if kind.has_editor() {
                menu.in_window()
            } else {
                menu
            };
            match kind.width() {
                Some(width) => menu.width(width),
                None => menu,
            }
        });
        self._menu.extend([
            cx.subscribe(&menu, move |this, menu, event: &menu::Event, cx| {
                let menu::Event::Chosen(action) = event;
                let value = menu.read(cx).chosen_value();
                this.chosen(kind, action, value, cx);
            }),
            cx.observe_release(&menu, |this, _, _| {
                this.menu = None;
                this.editors = Editors::default();
            }),
        ]);
        self.menu = Some((kind, menu.clone()));
        cx.emit(BarEvent::Menu(menu));
    }

    /// Creates the editors that the menu `kind` holds, with the values of
    /// `filters`, and subscribes to what they emit.
    fn create_editors(&mut self, kind: MenuKind, filters: &Filters, cx: &mut Context<Self>) {
        match kind {
            MenuKind::Color => {
                let editor = cx.new(|cx| {
                    ColorEditor::new(filters.color.clone(), filters.color_delta_e, true, cx)
                });
                self._menu.push(cx.subscribe(
                    &editor,
                    |this, _, ColorCommitted(color, delta_e): &ColorCommitted, cx| {
                        let (color, delta_e) = (color.clone(), *delta_e);
                        this.update(cx, |filters| {
                            filters.color = color;
                            filters.color_delta_e = delta_e;
                        });
                    },
                ));
                self.editors.color = Some(editor);
            }
            MenuKind::Imported | MenuKind::Modified => {
                let (after, before) = dates(kind, filters);
                let editor = cx.new(|_| DateRangeEditor::new(after, before));
                self._menu.push(cx.subscribe(
                    &editor,
                    move |this, _, DatesCommitted(after, before): &DatesCommitted, cx| {
                        let (after, before) = (*after, *before);
                        this.update(cx, |filters| set_dates(kind, filters, after, before));
                    },
                ));
                self.editors.dates = Some(editor);
            }
            MenuKind::Duration | MenuKind::Size => {
                let (min, max, units, unit) = if kind == MenuKind::Duration {
                    (
                        filters.min_duration_ms,
                        filters.max_duration_ms,
                        vec![(t("s"), 1000), (t("m"), 60_000), (t("h"), 3_600_000)],
                        0,
                    )
                } else {
                    (
                        filters.min_size_bytes,
                        filters.max_size_bytes,
                        vec![(t("KB"), 1000), (t("MB"), 1_000_000)],
                        1,
                    )
                };
                let editor = cx.new(|cx| RangeEditor::new(min, max, units, unit, cx));
                self._menu.push(cx.subscribe(
                    &editor,
                    move |this, _, RangeCommitted(min, max): &RangeCommitted, cx| {
                        let (min, max) = (*min, *max);
                        this.update(cx, |filters| {
                            if kind == MenuKind::Duration {
                                filters.min_duration_ms = min;
                                filters.max_duration_ms = max;
                            } else {
                                filters.min_size_bytes = min;
                                filters.max_size_bytes = max;
                            }
                        });
                    },
                ));
                self.editors.range = Some(editor);
            }
            MenuKind::Resolution => {
                let pixels = || vec![(t("px"), 1)];
                let width = cx.new(|cx| {
                    RangeEditor::new(filters.min_width, filters.max_width, pixels(), 0, cx)
                });
                let height = cx.new(|cx| {
                    RangeEditor::new(filters.min_height, filters.max_height, pixels(), 0, cx)
                });
                self._menu.push(cx.subscribe(
                    &width,
                    |this, _, RangeCommitted(min, max): &RangeCommitted, cx| {
                        let (min, max) = (*min, *max);
                        this.update(cx, |filters| {
                            filters.min_width = min;
                            filters.max_width = max;
                        });
                    },
                ));
                self._menu.push(cx.subscribe(
                    &height,
                    |this, _, RangeCommitted(min, max): &RangeCommitted, cx| {
                        let (min, max) = (*min, *max);
                        this.update(cx, |filters| {
                            filters.min_height = min;
                            filters.max_height = max;
                        });
                    },
                ));
                self.editors.resolution = Some(cx.new(|_| ResolutionEditor { width, height }));
            }
            MenuKind::Saved => {
                let savable = filters.count() > 0;
                let editor = cx.new(|cx| SaveFilterEditor::new(savable, cx));
                self._menu.push(cx.subscribe(
                    &editor,
                    |this, _, SaveRequested(name): &SaveRequested, cx| {
                        let filters = this.filters(cx);
                        if filters.count() > 0 {
                            this.saved.push((name.clone(), filters));
                            this.keep(cx);
                            this.refresh_menu(cx);
                        }
                    },
                ));
                self.editors.save = Some(editor);
            }
            MenuKind::Rating | MenuKind::Type | MenuKind::Pin => {}
        }
    }

    /// Updates the editors of the open menu to the filters as they are now,
    /// and builds the menu's rows again.
    fn refresh_menu(&mut self, cx: &mut Context<Self>) {
        let Some((kind, menu)) = self.menu.clone() else {
            return;
        };
        let filters = self.filters(cx);
        if let Some(editor) = &self.editors.color {
            let (color, delta_e) = (filters.color.clone(), filters.color_delta_e);
            editor.update(cx, |editor, cx| editor.set_value(color, delta_e, cx));
        }
        if let Some(editor) = &self.editors.dates {
            let (after, before) = dates(kind, &filters);
            editor.update(cx, |editor, cx| editor.set_range(after, before, cx));
        }
        if let Some(editor) = &self.editors.save {
            let savable = filters.count() > 0;
            editor.update(cx, |editor, cx| {
                editor.savable = savable;
                cx.notify();
            });
        }
        let entries = self.entries(kind, &filters);
        menu.update(cx, |menu, cx| menu.set_entries(entries, cx));
    }

    fn entries(&self, kind: MenuKind, filters: &Filters) -> Vec<menu::Entry> {
        // The width of the menu, which an editor fills.
        let width = kind.width().unwrap_or(270.);
        match kind {
            MenuKind::Rating => [5u8, 4, 3, 2, 1, 0]
                .into_iter()
                .map(|stars| {
                    let label = if stars == 0 {
                        t("Unrated").to_owned()
                    } else {
                        format!(
                            "{}{}",
                            "★".repeat(stars as usize),
                            "☆".repeat(5 - stars as usize)
                        )
                    };
                    menu::item("rating", label)
                        .value(stars.to_string())
                        .checked(filters.ratings.contains(&stars))
                        .keep_open(true)
                        .into()
                })
                .collect(),
            MenuKind::Type => {
                let (families, types) = filters::type_rows(&self.facets, filters);
                let row = |mime: &String| -> menu::Entry {
                    menu::item("type", crate::text::filter_type(mime))
                        .value(mime.clone())
                        .keywords([mime.clone()])
                        .checked(filters.include_mimes.contains(mime))
                        .excluded(filters.exclude_mimes.contains(mime))
                        .keep_open(true)
                        .context_action("type-exclude")
                        .into()
                };
                let mut entries = families.iter().map(row).collect::<Vec<_>>();
                if !families.is_empty() && !types.is_empty() {
                    entries.push(menu::Entry::Separator);
                }
                entries.extend(types.iter().map(row));
                entries
            }
            MenuKind::Pin => Field::ALL
                .into_iter()
                .map(|field| {
                    menu::item("pin", field.label())
                        .value(field.label())
                        .icon(pin_icon(field))
                        .checked(self.pinned.contains(&field))
                        .keep_open(true)
                        .into()
                })
                .collect(),
            MenuKind::Imported | MenuKind::Modified => {
                let mut entries = filters::DATE_PRESETS
                    .iter()
                    .enumerate()
                    .map(|(index, (label, _, _))| {
                        menu::item("date-preset", *label)
                            .value(index.to_string())
                            .into()
                    })
                    .collect::<Vec<menu::Entry>>();
                entries.push(menu::Entry::Separator);
                if let Some(editor) = &self.editors.dates {
                    entries.push(
                        menu::view(editor.clone(), width, filter_editors::DATE_HEIGHT)
                            .padded()
                            .into(),
                    );
                }
                entries
            }
            MenuKind::Color => self
                .editors
                .color
                .iter()
                .map(|editor| {
                    menu::view(editor.clone(), width, color_editor::height(true))
                        .padded()
                        .into()
                })
                .collect(),
            MenuKind::Duration | MenuKind::Size => self
                .editors
                .range
                .iter()
                .map(|editor| {
                    menu::view(editor.clone(), width, filter_editors::RANGE_HEIGHT)
                        .padded()
                        .into()
                })
                .collect(),
            MenuKind::Resolution => self
                .editors
                .resolution
                .iter()
                .map(|editor| {
                    menu::view(editor.clone(), width, filter_editors::RESOLUTION_HEIGHT)
                        .padded()
                        .into()
                })
                .collect(),
            MenuKind::Saved => {
                let mut entries = self
                    .editors
                    .save
                    .iter()
                    .map(|editor| {
                        menu::view(editor.clone(), width, filter_editors::SAVE_HEIGHT)
                            .padded()
                            .into()
                    })
                    .collect::<Vec<menu::Entry>>();
                if !self.saved.is_empty() {
                    entries.push(menu::Entry::Separator);
                }
                entries.extend(self.saved.iter().enumerate().map(|(index, (name, _))| {
                    menu::item("saved", name.clone())
                        .value(index.to_string())
                        .context_action("saved-delete")
                        .into()
                }));
                entries
            }
        }
    }

    fn chosen(
        &mut self,
        kind: MenuKind,
        action: &str,
        value: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let value = value.map(|value| value.to_string()).unwrap_or_default();
        match action {
            "rating" => {
                if let Ok(stars) = value.parse::<u8>() {
                    self.update(cx, |filters| filters.toggle_rating(stars));
                }
            }
            "type" => self.update(cx, |filters| filters.toggle_mime(&value)),
            "type-exclude" => self.update(cx, |filters| filters.toggle_excluded_mime(&value)),
            "pin" => {
                if let Some(field) = Field::ALL.into_iter().find(|field| field.label() == value) {
                    match self.pinned.iter().position(|pinned| *pinned == field) {
                        Some(at) => {
                            self.pinned.remove(at);
                        }
                        None => self.pinned.push(field),
                    }
                    self.keep(cx);
                    self.refresh_menu(cx);
                    cx.notify();
                }
            }
            "date-preset" => {
                if let Some((_, days, end)) = value
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| filters::DATE_PRESETS.get(index))
                {
                    let now = crate::now_ms();
                    let (after, before) = filters::date_preset(now, *days, *end);
                    self.update(cx, |filters| {
                        set_dates(kind, filters, Some(after), Some(before))
                    });
                }
            }
            "saved" => {
                if let Some((_, saved)) = value
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| self.saved.get(index))
                {
                    let saved = saved.clone();
                    self.session
                        .update(cx, |session, cx| session.set_filters(saved, cx));
                }
            }
            "saved-delete" => {
                if let Ok(index) = value.parse::<usize>()
                    && index < self.saved.len()
                {
                    self.saved.remove(index);
                    self.keep(cx);
                    self.refresh_menu(cx);
                }
            }
            _ => {}
        }
    }

    /// Returns the tag picker and the folder picker, which the window draws
    /// over everything else.
    pub fn pickers(&self) -> (Entity<TagPicker>, Entity<FolderPicker>) {
        (self.tag_picker.clone(), self.folder_picker.clone())
    }

    /// Opens the tag picker or the folder picker in its filter mode, 4 px
    /// below the control whose bounds are `anchor`.
    fn open_picker(
        &mut self,
        field: Field,
        anchor: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, menu)) = self.menu.take() {
            menu.update(cx, |menu, cx| menu.dismiss(cx));
        }
        let below = point(f32::from(anchor.left()), f32::from(anchor.bottom()) + 4.);
        let filters = self.filters(cx);
        if field == Field::Tags {
            let records = {
                let session = self.session.read(cx);
                move |choices: &[TagChoice]| {
                    choices
                        .iter()
                        .map(|choice| match session.tags.get(&choice.tag_id) {
                            Some(record) => record.clone(),
                            None => picto_library::TagRecord {
                                tag_id: picto_library::TagId(choice.tag_id),
                                ..crate::tags::record(&choice.name)
                            },
                        })
                        .collect::<Vec<_>>()
                }
            };
            let (include, exclude) = (
                records(&filters.include_tags),
                records(&filters.exclude_tags),
            );
            self.tag_picker.update(cx, |picker, cx| {
                picker.open_filter(below, include, exclude, filters.tag_mode, window, cx)
            });
        } else {
            let ids = |folders: &[u32]| {
                folders
                    .iter()
                    .map(|id| picto_library::FolderId(*id))
                    .collect::<Vec<_>>()
            };
            let (include, exclude) = (ids(&filters.include_folders), ids(&filters.exclude_folders));
            self.folder_picker.update(cx, |picker, cx| {
                picker.open_filter(below, include, exclude, filters.folder_mode, window, cx)
            });
        }
    }

    // The functions below draw the bar.

    fn control(&self, field: Field, filters: &Filters, cx: &mut Context<Self>) -> AnyElement {
        let names = {
            let session = self.session.read(cx);
            move |id: u32| {
                session
                    .nav
                    .iter()
                    .find(|entry| {
                        matches!(entry.scope, ItemScope::FolderTree { folder_id } | ItemScope::Folder { folder_id } if folder_id.0 == id)
                    })
                    .map(|entry| entry.label.clone())
            }
        };
        let label = filters.label(field, names);
        let active = filters.is_set(field);
        let glyph = match (&filters.color, field) {
            (Some(color), Field::Color) => Glyph::Color(rgb(theme::parse_hex_code(
                &color.to_uppercase(),
            )
            .unwrap_or(0x808080))),
            _ => Glyph::Icon(bar_icon(field)),
        };
        let (open, clear) = (cx.entity().downgrade(), cx.entity().downgrade());
        filter_control(("filter-field", field as usize), glyph, label, active)
            .on_open(move |bounds, window, cx| {
                let _ = open.update(cx, |this, cx| match field {
                    Field::Tags | Field::Folders => this.open_picker(field, bounds, window, cx),
                    field => {
                        if let Some(kind) = menu_kind(field) {
                            this.open_menu(kind, bounds, cx)
                        }
                    }
                });
            })
            .on_clear(move |_, cx| {
                let _ = clear.update(cx, |this, cx| {
                    this.update(cx, |filters| filters.clear(field))
                });
            })
            .into_any_element()
    }
}

/// Returns the menu that a field's control opens, or `None` for a field
/// whose control opens a picker.
fn menu_kind(field: Field) -> Option<MenuKind> {
    Some(match field {
        Field::Color => MenuKind::Color,
        Field::Rating => MenuKind::Rating,
        Field::Type => MenuKind::Type,
        Field::Imported => MenuKind::Imported,
        Field::Modified => MenuKind::Modified,
        Field::Resolution => MenuKind::Resolution,
        Field::Duration => MenuKind::Duration,
        Field::Size => MenuKind::Size,
        // The controls of tags and folders open their pickers instead.
        Field::Tags | Field::Folders => return None,
    })
}

/// Returns the name of the 16 px icon that a field's control shows on the
/// bar.
fn bar_icon(field: Field) -> &'static str {
    match field {
        Field::Color => "filter-color",
        Field::Tags => "filter-bookmark",
        Field::Folders => "filter-folder",
        Field::Rating => "filter-star",
        Field::Type => "filter-photo",
        Field::Imported | Field::Modified => "filter-calendar",
        Field::Resolution => "filter-dimensions",
        Field::Duration => "filter-clock",
        Field::Size => "filter-file",
    }
}

/// Returns the name of the icon that a field's row shows in the pin menu.
fn pin_icon(field: Field) -> &'static str {
    match field {
        Field::Color => "pin-color",
        Field::Tags => "pin-bookmark",
        Field::Folders => "pin-folder",
        Field::Rating => "star",
        Field::Type => "pin-photo",
        Field::Imported | Field::Modified => "pin-calendar",
        Field::Resolution => "pin-dimensions",
        Field::Duration => "pin-clock",
        Field::Size => "pin-file",
    }
}

fn dates(kind: MenuKind, filters: &Filters) -> (Option<i64>, Option<i64>) {
    if kind == MenuKind::Modified {
        (filters.modified_after, filters.modified_before)
    } else {
        (filters.imported_after, filters.imported_before)
    }
}

fn set_dates(kind: MenuKind, filters: &mut Filters, after: Option<i64>, before: Option<i64>) {
    if kind == MenuKind::Modified {
        filters.modified_after = after;
        filters.modified_before = before;
    } else {
        filters.imported_after = after;
        filters.imported_before = before;
    }
}

impl Render for FilterBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (filters, locked) = {
            let session = self.session.read(cx);
            (session.filters.clone(), session.filters_locked)
        };
        let count = filters.count();
        let mut items = div()
            .id("filter-items")
            .flex()
            .items_start()
            .w_full()
            .h(px(32.))
            .pt(px(6.))
            .pr(px(18.))
            .pl(px(10.))
            .overflow_x_scroll()
            .track_scroll(&self.scroll);
        for field in Field::ALL {
            if self.pinned.contains(&field) || filters.is_set(field) {
                items = items.child(self.control(field, &filters, cx));
            }
        }
        let pin = cx.entity().downgrade();
        items = items.child(
            filter_action("filter-add", "filter-plus")
                .name(t("Add filter"))
                .tip(ui::tip(t("Add or remove filter fields")))
                .on_click(move |bounds, _, cx| {
                    let _ = pin.update(cx, |this, cx| this.open_menu(MenuKind::Pin, bounds, cx));
                }),
        );
        let background = theme.background();
        let viewport =
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .h(px(32.))
                .ml(px(-10.))
                .child(items)
                // A gradient at each end, from the bar's background to
                // transparent, is drawn over the controls that scroll under it.
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .w(px(10.))
                        .h(px(24.))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(background, 0.),
                            linear_color_stop(theme::clear(background), 1.),
                        )),
                )
                .child(div().absolute().right_0().top_0().w(px(18.)).h(px(24.)).bg(
                    linear_gradient(
                        270.,
                        linear_color_stop(background, 0.),
                        linear_color_stop(theme::clear(background), 1.),
                    ),
                ));
        let (saved, lock, reset) = (
            cx.entity().downgrade(),
            cx.entity().downgrade(),
            cx.entity().downgrade(),
        );
        let right = div()
            .flex()
            .items_start()
            .flex_none()
            .h(px(32.))
            .pt(px(6.))
            .gap(px(2.))
            .child(
                div()
                    .w(px(1.))
                    .h(px(18.))
                    .mt(px(3.))
                    .mr(px(6.))
                    .bg(theme.border_secondary()),
            )
            .child(
                filter_action("filter-saved", "filter-floppy")
                    .name(t("Saved filters"))
                    .tip(ui::tip(t("Save or apply a filter")))
                    .on_click(move |bounds, _, cx| {
                        let _ = saved
                            .update(cx, |this, cx| this.open_menu(MenuKind::Saved, bounds, cx));
                    }),
            )
            .child(
                filter_action(
                    "filter-lock",
                    if locked {
                        "filter-lock"
                    } else {
                        "filter-unlock"
                    },
                )
                .active(locked)
                .name(if locked {
                    t("Unlock filters")
                } else {
                    t("Lock filters")
                })
                .pressed(locked)
                .tip(ui::tip(if locked {
                    t("Keep filters on navigation: on")
                } else {
                    t("Keep filters on navigation")
                }))
                .on_click(move |_, _, cx| {
                    let _ = lock.update(cx, |this, cx| {
                        this.session
                            .update(cx, |session, cx| session.set_filters_locked(!locked, cx));
                    });
                }),
            )
            .child(
                filter_action("filter-clear-all", "filter-restore")
                    .name(t("Clear filters"))
                    .disabled(count == 0)
                    .tip(ui::tip(t("Clear all filters")))
                    .on_click(move |_, _, cx| {
                        let _ = reset.update(cx, |this, cx| {
                            this.session.update(cx, |session, cx| {
                                session.set_filters(Filters::default(), cx)
                            });
                        });
                    }),
            );
        div()
            .relative()
            .flex_none()
            .min_h(px(HEIGHT))
            .pt(px(6.))
            .px(px(12.))
            .flex()
            .items_start()
            .gap(px(8.))
            .text(theme::TEXT_MD)
            .line_height(px(theme::LINE_HEIGHT_MD))
            .text_color(theme.text())
            .child(viewport)
            .child(right)
    }
}
