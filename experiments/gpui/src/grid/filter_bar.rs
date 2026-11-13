//! The filter bar under the titlebar (`GridFilterToolbar` in
//! `GridFilterMenu.tsx`): a control for each pinned or set filter, each
//! opening its menu or picker, the button that pins fields, and on the
//! right the saved filters, the navigation lock and Clear.
use super::filter_editors::{
    self, DateRangeEditor, DatesCommitted, RangeCommitted, RangeEditor, ResolutionEditor,
    SaveFilterEditor, SaveRequested,
};
use super::filters::{self, Field, Filters, TagChoice};
use crate::inspector::{
    FolderPicker, TagPicker,
    folder_picker::Event as FolderEvent,
    tag_picker::{self, Event as TagEvent},
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

/// `.toolbar`'s `min-height`.
pub const HEIGHT: f32 = 38.;

/// The bar asks the window to show a menu above everything.
pub enum BarEvent {
    Menu(Entity<Menu>),
}

impl EventEmitter<BarEvent> for FilterBar {}

/// Which menu is open (`FilterMenuKind`).
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

    /// `activeMenuWidth`; the others keep the menu's 270px.
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

/// The editors of the open menu, kept so the rows can be rebuilt around
/// them and so they follow filters changed elsewhere.
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
    /// The fields shown while empty (`pinnedFilters`), in no order: the bar
    /// keeps its own.
    pinned: Vec<Field>,
    /// Saved filters, by name (`savedFilters`).
    saved: Vec<(String, Filters)>,
    /// The types the rest of the filters leave, for the type menu.
    facets: Vec<String>,
    /// What the facets were asked for: destination, search and filters.
    facet_query: Option<(usize, String, Filters)>,
    facet_task: Option<Task<()>>,
    menu: Option<(MenuKind, Entity<Menu>)>,
    editors: Editors,
    /// The tag and folder pickers in their filter mode, which the window
    /// draws over everything.
    tag_picker: Entity<TagPicker>,
    folder_picker: Entity<FolderPicker>,
    /// What the bar last showed, so it repaints only when that changes.
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
        // The pinned fields and saved filters the user left
        // (`loadPinnedFilters`, `loadSavedFilters`).
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

    /// Keeps the pinned fields and saved filters in the app's state, as
    /// production keeps them in localStorage.
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

    /// The filters, the bar or the destination changed: repaint if what the
    /// bar shows changed, keep the open menu's rows and editors in step,
    /// and fetch the type facets again when they may differ.
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

    /// `queryMimeFacets` for the destination with every filter but the
    /// types, while the bar is open.
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

    // ── Menus ───────────────────────────────────────────────────

    fn open_menu(&mut self, kind: MenuKind, anchor: Bounds<Pixels>, cx: &mut Context<Self>) {
        if let Some((_, menu)) = self.menu.take() {
            menu.update(cx, |menu, cx| menu.dismiss(cx));
        }
        self.editors = Editors::default();
        self._menu.clear();
        let filters = self.filters(cx);
        self.create_editors(kind, &filters, cx);
        let entries = self.entries(kind, &filters, cx);
        // `{ x: rect.left, y: rect.bottom + 4 }`.
        let position = point(anchor.left(), anchor.bottom() + px(4.));
        let menu = cx.new(|cx| {
            let menu = Menu::new(entries, position, cx)
                .search(matches!(kind, MenuKind::Type | MenuKind::Pin));
            // Editors take the keyboard, and their selects and calendars
            // open over the window.
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

    /// The editors a menu holds, made from the filters as they are.
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
                        vec![("s", 1000), ("m", 60_000), ("h", 3_600_000)],
                        0,
                    )
                } else {
                    (
                        filters.min_size_bytes,
                        filters.max_size_bytes,
                        vec![("KB", 1000), ("MB", 1_000_000)],
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
                let pixels = || vec![("px", 1)];
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

    /// The open menu's rows for the filters as they are now, and its
    /// editors following them.
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
        let entries = self.entries(kind, &filters, cx);
        menu.update(cx, |menu, cx| menu.set_entries(entries, cx));
    }

    fn entries(&self, kind: MenuKind, filters: &Filters, cx: &App) -> Vec<menu::Entry> {
        // The menu's width, which an editor fills.
        let width = kind.width().unwrap_or(270.);
        match kind {
            MenuKind::Rating => [5u8, 4, 3, 2, 1, 0]
                .into_iter()
                .map(|stars| {
                    let label = if stars == 0 {
                        "Unrated".to_owned()
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
                let _ = cx;
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

    /// The tag and folder pickers, which the window draws over everything.
    pub fn pickers(&self) -> (Entity<TagPicker>, Entity<FolderPicker>) {
        (self.tag_picker.clone(), self.folder_picker.clone())
    }

    /// Opens the tag or folder picker in its filter mode, 4px below the
    /// control.
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
                                ..tag_picker::record(&choice.name)
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

    // ── Drawing ─────────────────────────────────────────────────

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
                    field => this.open_menu(menu_kind(field), bounds, cx),
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

/// The menu a field's control opens.
fn menu_kind(field: Field) -> MenuKind {
    match field {
        Field::Color => MenuKind::Color,
        Field::Rating => MenuKind::Rating,
        Field::Type => MenuKind::Type,
        Field::Imported => MenuKind::Imported,
        Field::Modified => MenuKind::Modified,
        Field::Resolution => MenuKind::Resolution,
        Field::Duration => MenuKind::Duration,
        Field::Size => MenuKind::Size,
        // Tags and folders open their pickers instead.
        Field::Tags | Field::Folders => MenuKind::Pin,
    }
}

/// A field's 16px glyph on the bar (`stroke={1.6}`).
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

/// A field's glyph in the pin menu.
fn pin_icon(field: Field) -> &'static str {
    match field {
        Field::Color => "pin-color",
        Field::Tags => "pin-bookmark",
        Field::Folders => "pin-folder",
        Field::Rating => "pin-star",
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
                .tip(ui::tip("Add or remove filter fields"))
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
                // The fades at either end, over the controls that scroll under
                // them.
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
                    .tip(ui::tip("Save or apply a filter"))
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
                .tip(ui::tip(if locked {
                    "Keep filters on navigation: on"
                } else {
                    "Keep filters on navigation"
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
                    .disabled(count == 0)
                    .tip(ui::tip("Clear all filters"))
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
