//! The menu's pure arithmetic: the rows as drawn, search and the arrow keys'
//! steps, row heights, and where panels and submenus are placed.
use super::{
    EMPTY_HEIGHT, Entry, GAP, MARGIN, PADDING, SEPARATOR, SUBMENU_GAP, SUBMENU_LIFT, SWATCH_ROW,
};
use crate::theme;
use gpui::*;

/// Where an entry is: a row of the menu, or a row of one of its submenus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Path {
    pub(super) top: usize,
    pub(super) child: Option<usize>,
}

/// One line of a panel as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Entry(Path),
    Separator,
    /// A swatch row: not a stop for the arrow keys, nor found by a search.
    Swatches(Path),
    /// A view entry and its height in pixels.
    View(Path, u32),
}

/// A rectangle in the coordinates of the window or display it is placed in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Rect {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) w: f32,
    pub(super) h: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub(super) fn outset(&self, by: f32) -> Rect {
        Rect {
            x: self.x - by,
            y: self.y - by,
            w: self.w + 2. * by,
            h: self.h + 2. * by,
        }
    }
    pub(super) fn intersect(&self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Rect {
            x,
            y,
            w: (self.right().min(other.right()) - x).max(0.),
            h: (self.bottom().min(other.bottom()) - y).max(0.),
        }
    }
    /// A submenu's window: its `panel` with `room` around it for the drawn
    /// shadow, except on the side of the menu it opened from. A window takes
    /// the pointer over all of itself, so room laid over the parent would
    /// leave the parent's edge, where its arrows are, dead to the pointer.
    pub(super) fn submenu_window(panel: Rect, parent: Rect, room: f32, area: Rect) -> Rect {
        let mut window = panel.outset(room).intersect(area);
        if panel.x >= parent.right() {
            let left = window.x.max(parent.right());
            window.w = (window.right() - left).max(0.);
            window.x = left;
        } else if panel.right() <= parent.x {
            window.w = (parent.x - window.x).clamp(0., window.w);
        }
        window
    }
    pub(super) fn from_bounds(bounds: Bounds<Pixels>) -> Rect {
        Rect {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            w: f32::from(bounds.size.width),
            h: f32::from(bounds.size.height),
        }
    }
    pub(super) fn to_bounds(self) -> Bounds<Pixels> {
        Bounds::new(point(px(self.x), px(self.y)), size(px(self.w), px(self.h)))
    }
}

/// `placeMenu`: the panel's corner at the pointer, moved inward so the
/// panel keeps the margin from every edge of the area. The height is the
/// panel's full height, before any maximum.
pub(super) fn place(pointer: (f32, f32), w: f32, h: f32, area: Rect) -> (f32, f32) {
    let (mut x, mut y) = pointer;
    if x + w > area.right() - MARGIN {
        x = area.right() - w - MARGIN;
    }
    if y + h > area.bottom() - MARGIN {
        y = area.bottom() - h - MARGIN;
    }
    (x.max(area.x + MARGIN), y.max(area.y + MARGIN))
}

/// `placeMenu`'s `transform-origin` for the panel `place` puts there: the
/// corner at the pointer, or the right or bottom edge where the panel was
/// moved inward from that edge. Fractions of the width and height.
pub(super) fn origin(pointer: (f32, f32), w: f32, h: f32, area: Rect) -> (f32, f32) {
    (
        if pointer.0 + w > area.right() - MARGIN {
            1.
        } else {
            0.
        },
        if pointer.1 + h > area.bottom() - MARGIN {
            1.
        } else {
            0.
        },
    )
}

/// `SubmenuPanel`'s placement: 4px right of the parent panel and 3px above
/// its row, on the parent's left instead when it would cross the margin on
/// the right, then moved up or down to keep the margin.
pub(super) fn place_submenu(parent: Rect, row_top: f32, w: f32, h: f32, area: Rect) -> (f32, f32) {
    let mut x = parent.right() + SUBMENU_GAP;
    let mut y = row_top - SUBMENU_LIFT;
    if x + w > area.right() - MARGIN {
        x = parent.x - w - SUBMENU_GAP;
    }
    if y + h > area.bottom() - MARGIN {
        y = area.bottom() - h - MARGIN;
    }
    (x, y.max(area.y + MARGIN))
}

/// `cleanSeparators`: no separator first, last or twice in a row. A
/// submenu's rows leave out nested submenus, which are not drawn.
pub(super) fn clean(entries: &[Entry], parent: Option<usize>) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let path = match parent {
            Some(top) => Path {
                top,
                child: Some(index),
            },
            None => Path {
                top: index,
                child: None,
            },
        };
        match entry {
            Entry::Separator => {
                // After any row, a swatch row or a view included.
                if rows.last().is_some_and(|row| *row != Row::Separator) {
                    rows.push(Row::Separator);
                }
            }
            Entry::Submenu(_) if parent.is_some() => {}
            Entry::Swatches(_) => rows.push(Row::Swatches(path)),
            Entry::View(view) => rows.push(Row::View(path, view.height.round() as u32)),
            _ => rows.push(Row::Entry(path)),
        }
    }
    if rows.last() == Some(&Row::Separator) {
        rows.pop();
    }
    rows
}

/// `menuSearchScore`: a label that starts with the query, then one that
/// contains it, then the label and keywords containing it, then the query's
/// letters in order with the fewest gaps. Lower is better.
fn score(text: &str, label: &str, query: &str) -> Option<usize> {
    if label.starts_with(query) {
        return Some(0);
    }
    if let Some(at) = label.find(query) {
        return Some(10 + label[..at].chars().count());
    }
    if let Some(at) = text.find(query) {
        return Some(100 + text[..at].chars().count());
    }
    let query = query.chars().collect::<Vec<_>>();
    let (mut found, mut gap) = (0, 0);
    for letter in text.chars() {
        if found == query.len() {
            break;
        }
        if letter == query[found] {
            found += 1;
        } else if found > 0 {
            gap += 1;
        }
    }
    (found == query.len()).then_some(200 + gap)
}

/// `searchMenuEntries`: every enabled item and submenu whose label or
/// keywords match, best first and in menu order among equals. Submenu rows
/// are searched too, so a match inside one is a row that can be chosen
/// directly.
pub(super) fn search(entries: &[Entry], raw: &str) -> Vec<Path> {
    let query = raw.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let matching = |label: &SharedString, keywords: &[SharedString]| {
        let label = label.to_lowercase();
        let text = format!(
            "{label} {}",
            keywords
                .iter()
                .map(|word| word.as_ref())
                .collect::<Vec<_>>()
                .join(" ")
        )
        .to_lowercase();
        score(&text, &label, &query)
    };
    let mut matches = Vec::new();
    let mut order = 0;
    let mut visit = |entry: &Entry, path: Path| {
        let found = match entry {
            Entry::Separator | Entry::Swatches(_) | Entry::View(_) => return,
            Entry::Item(item) if item.disabled => None,
            Entry::Item(item) => matching(&item.label, &item.keywords),
            Entry::Submenu(submenu) => matching(&submenu.label, &[]),
        };
        if let Some(score) = found {
            matches.push((score, order, path));
        }
        order += 1;
    };
    for (top, entry) in entries.iter().enumerate() {
        visit(entry, Path { top, child: None });
        if let Entry::Submenu(submenu) = entry {
            for (child, entry) in submenu.children.iter().enumerate() {
                // A submenu inside a submenu has no row to show it.
                if !matches!(entry, Entry::Submenu(_)) {
                    visit(
                        entry,
                        Path {
                            top,
                            child: Some(child),
                        },
                    );
                }
            }
        }
    }
    matches.sort_by_key(|(score, order, _)| (*score, *order));
    matches.into_iter().map(|(_, _, path)| path).collect()
}

/// The row the arrow keys reach next among the rows that can be stopped
/// on, wrapping at the ends.
pub(super) fn step(stops: &[bool], current: Option<usize>, forward: bool) -> Option<usize> {
    let rows = (0..stops.len())
        .filter(|index| stops[*index])
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return current;
    }
    let at = current.and_then(|current| rows.iter().position(|row| *row == current));
    let next = match (at, forward) {
        (Some(at), true) => (at + 1) % rows.len(),
        (Some(at), false) => (at + rows.len() - 1) % rows.len(),
        (None, true) => 0,
        (None, false) => rows.len() - 1,
    };
    Some(rows[next])
}

fn row_height(row: &Row) -> f32 {
    match row {
        Row::Entry(_) => theme::CONTROL_SM,
        Row::Separator => SEPARATOR,
        Row::Swatches(_) => SWATCH_ROW,
        Row::View(_, height) => *height as f32,
    }
}

/// `.items`: its padding, rows and gaps, or the empty-result line.
pub(super) fn items_height(rows: &[Row], empty_line: bool) -> f32 {
    if rows.is_empty() {
        return 2. * PADDING + if empty_line { EMPTY_HEIGHT } else { 0. };
    }
    2. * PADDING + rows.iter().map(row_height).sum::<f32>() + GAP * (rows.len() - 1) as f32
}

/// Where a row starts below the top of its panel's items area.
pub(super) fn row_offset(rows: &[Row], index: usize) -> f32 {
    PADDING
        + rows[..index.min(rows.len())]
            .iter()
            .map(|row| row_height(row) + GAP)
            .sum::<f32>()
}

#[cfg(test)]
mod tests {
    use super::{
        Entry, MARGIN, PADDING, Path, Rect, Row, SEPARATOR, clean, items_height, origin, place,
        place_submenu, row_offset, score, search, step,
    };
    use crate::ui::menu::{item, submenu, swatches};

    fn top(index: usize) -> Path {
        Path {
            top: index,
            child: None,
        }
    }

    fn child(top: usize, child: usize) -> Path {
        Path {
            top,
            child: Some(child),
        }
    }

    fn grid_like() -> Vec<Entry> {
        vec![
            item("open", "Open with Default App").into(),
            item("reveal", "Reveal in Finder")
                .keywords(["show", "folder"])
                .into(),
            Entry::Separator,
            item("copy-path", "Copy File Path").into(),
            item("copy-name", "Copy Name").into(),
            Entry::Separator,
            submenu(
                "Set Rating",
                vec![
                    item("rate-0", "No Rating").into(),
                    item("rate-1", "★").into(),
                ],
            )
            .icon("menu-star")
            .into(),
            item("delete", "Delete Permanently").disabled(true).into(),
            item("trash", "Move to Trash").into(),
        ]
    }

    #[test]
    fn search_ranks_label_starts_then_contains_then_keywords_then_letters() {
        let entries = grid_like();
        // "co": both copies start with it; earlier in the menu first.
        assert_eq!(search(&entries, "co"), vec![top(3), top(4)]);
        // "name": inside a label.
        assert_eq!(search(&entries, " Name "), vec![top(4)]);
        // "folder": a keyword only.
        assert_eq!(search(&entries, "folder"), vec![top(1)]);
        // "mvt": the letters of "Move to Trash" in order, with gaps.
        assert_eq!(search(&entries, "mvt"), vec![top(8)]);
        assert_eq!(score("move to trash", "move to trash", "move"), Some(0));
        assert_eq!(score("copy name", "copy name", "name"), Some(15));
        assert_eq!(
            score("reveal in finder show", "reveal in finder", "show"),
            Some(117)
        );
        assert_eq!(score("abc", "abc", "xyz"), None);
    }

    #[test]
    fn search_skips_disabled_items_and_finds_submenus_and_their_rows() {
        let entries = grid_like();
        // The disabled delete row never matches.
        assert_eq!(search(&entries, "delete"), Vec::<Path>::new());
        // A submenu's own row and its children are both results.
        assert_eq!(search(&entries, "rating"), vec![child(6, 0), top(6)]);
        assert_eq!(search(&entries, "set"), vec![top(6)]);
        assert_eq!(search(&entries, "★"), vec![child(6, 1)]);
        assert_eq!(search(&entries, "   "), Vec::<Path>::new());
    }

    #[test]
    fn separators_never_lead_trail_or_double() {
        let entries: Vec<Entry> = vec![
            Entry::Separator,
            item("a", "A").into(),
            Entry::Separator,
            Entry::Separator,
            item("b", "B").into(),
            Entry::Separator,
        ];
        assert_eq!(
            clean(&entries, None),
            vec![Row::Entry(top(1)), Row::Separator, Row::Entry(top(4))]
        );
    }

    #[test]
    fn a_swatch_row_keeps_the_separator_after_it() {
        let entries: Vec<Entry> = vec![
            item("a", "A").into(),
            swatches("color", Vec::new(), None).into(),
            Entry::Separator,
            item("b", "B").into(),
        ];
        assert_eq!(
            clean(&entries, None),
            vec![
                Row::Entry(top(0)),
                Row::Swatches(top(1)),
                Row::Separator,
                Row::Entry(top(3))
            ]
        );
    }

    #[test]
    fn arrows_step_over_separators_and_disabled_rows_and_stop_on_submenus() {
        // open, reveal, ─, copy path, copy name, ─, Set Rating ›, delete (disabled), trash
        let stops = [true, true, false, true, true, false, true, false, true];
        assert_eq!(step(&stops, None, true), Some(0));
        assert_eq!(step(&stops, None, false), Some(8));
        assert_eq!(step(&stops, Some(1), true), Some(3));
        assert_eq!(step(&stops, Some(4), true), Some(6));
        assert_eq!(step(&stops, Some(6), true), Some(8));
        assert_eq!(step(&stops, Some(8), true), Some(0));
        assert_eq!(step(&stops, Some(0), false), Some(8));
        assert_eq!(step(&stops, Some(8), false), Some(6));
        // Nothing to stop on leaves the highlight where it was.
        assert_eq!(step(&[false, false], Some(1), true), Some(1));
    }

    #[test]
    fn heights_count_rows_separators_gaps_and_the_empty_line() {
        let rows = [Row::Entry(top(0)), Row::Separator, Row::Entry(top(2))];
        assert_eq!(
            items_height(&rows, false),
            2. * PADDING + 26. + SEPARATOR + 26. + 2. * 2.
        );
        assert_eq!(items_height(&[], true), 2. * PADDING + 30.);
        assert_eq!(row_offset(&rows, 2), PADDING + 26. + 2. + SEPARATOR + 2.);
    }

    #[test]
    fn a_menu_grows_from_the_corner_it_was_placed_by() {
        let area = Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 800.,
        };
        // Room at the pointer: from the top-left corner, at the pointer.
        assert_eq!(origin((100., 100.), 270., 300., area), (0., 0.));
        // Moved in from the right edge: from the right; from the bottom too.
        assert_eq!(origin((900., 100.), 270., 300., area), (1., 0.));
        assert_eq!(origin((900., 700.), 270., 300., area), (1., 1.));
        // Exactly at the margin still fits.
        assert_eq!(
            origin(
                (1000. - 270. - MARGIN, 800. - 300. - MARGIN),
                270.,
                300.,
                area
            ),
            (0., 0.)
        );
        // The scaled frame then keeps that corner where `place` put it.
        let (x, y) = place((900., 700.), 270., 300., area);
        let (dx, dy, w, h) = crate::motion::scaled_frame(270., 300., 0.96, (1., 1.));
        assert!((x + dx + w - (x + 270.)).abs() < 1e-3);
        assert!((y + dy + h - (y + 300.)).abs() < 1e-3);
    }

    #[test]
    fn a_menu_moves_inward_to_keep_its_margin() {
        let area = Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 800.,
        };
        // Room at the pointer.
        assert_eq!(place((100., 100.), 270., 300., area), (100., 100.));
        // Near the right and bottom edges it moves left and up.
        assert_eq!(
            place((900., 700.), 270., 300., area),
            (1000. - 270. - MARGIN, 800. - 300. - MARGIN)
        );
        // Taller than the area: held at the top margin.
        assert_eq!(place((50., 50.), 270., 900., area).1, MARGIN);
        // An area that does not start at zero, like a display's usable part
        // below the menu bar.
        let display = Rect {
            x: 0.,
            y: 25.,
            w: 1000.,
            h: 775.,
        };
        assert_eq!(place((5., 0.), 270., 100., display), (MARGIN, 25. + MARGIN));
    }

    #[test]
    fn a_submenu_opens_right_of_its_row_and_flips_left_at_the_edge() {
        let area = Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 800.,
        };
        let parent = Rect {
            x: 100.,
            y: 100.,
            w: 270.,
            h: 400.,
        };
        assert_eq!(
            place_submenu(parent, 160., 150., 200., area),
            (100. + 270. + 4., 157.)
        );
        // Too close to the right edge: on the parent's left.
        let right = Rect { x: 700., ..parent };
        assert_eq!(
            place_submenu(right, 160., 150., 200., area),
            (700. - 150. - 4., 157.)
        );
        // Low rows move up to keep the margin; high ones down.
        assert_eq!(
            place_submenu(parent, 700., 150., 200., area).1,
            800. - 200. - MARGIN
        );
        assert_eq!(place_submenu(parent, 5., 150., 200., area).1, MARGIN);
    }

    #[test]
    fn a_submenu_window_never_lies_over_the_menu_it_opened_from() {
        let area = Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 800.,
        };
        let parent = Rect {
            x: 100.,
            y: 100.,
            w: 270.,
            h: 400.,
        };
        // To the right: the window starts at the parent's edge, and keeps
        // its room on the other three sides.
        let panel = Rect {
            x: 374.,
            y: 157.,
            w: 150.,
            h: 200.,
        };
        let window = Rect::submenu_window(panel, parent, 40., area);
        assert_eq!((window.x, window.y), (370., 117.));
        assert_eq!((window.right(), window.bottom()), (564., 397.));
        // To the left: it ends at the parent's edge.
        let parent = Rect { x: 700., ..parent };
        let panel = Rect { x: 546., ..panel };
        let window = Rect::submenu_window(panel, parent, 40., area);
        assert_eq!((window.x, window.right()), (506., 700.));
    }

    #[test]
    fn rects_grow_for_shadows_and_clip_to_the_display() {
        let panel = Rect {
            x: 10.,
            y: 20.,
            w: 100.,
            h: 50.,
        };
        let area = Rect {
            x: 0.,
            y: 25.,
            w: 1000.,
            h: 800.,
        };
        assert_eq!(
            panel.outset(40.).intersect(area),
            Rect {
                x: 0.,
                y: 25.,
                w: 150.,
                h: 85.,
            }
        );
    }
}
