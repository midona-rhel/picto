//! The font viewer (`FontViewer.tsx`): the file loaded into GPUI's text
//! system, then a page with its name, four modes (a sample, a waterfall of
//! sizes, the glyphs, and facts about the file), five paper colours and a
//! size control floating at the bottom.
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::text_input::{Face, Kind, TextInput};
use crate::{theme, ui};
use anyhow::{Result, anyhow, bail};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::borrow::Cow;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Preview,
    Waterfall,
    Glyphs,
    Information,
}

// i18n: translated where drawn
const TABS: [(Tab, &str); 4] = [
    (Tab::Preview, "Preview"),
    (Tab::Waterfall, "Waterfall"),
    (Tab::Glyphs, "Glyphs"),
    (Tab::Information, "Information"),
];

/// The page's colours (`data-preview-theme`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Paper {
    Auto,
    Light,
    Dark,
    Purple,
    Yellow,
}

const PAPERS: [Paper; 5] = [
    Paper::Auto,
    Paper::Light,
    Paper::Dark,
    Paper::Purple,
    Paper::Yellow,
];

const WATERFALL_SIZES: [f32; 11] = [72., 64., 56., 48., 38., 30., 24., 20., 16., 14., 12.];
// i18n: translated where drawn (the groups' names; the glyphs are samples)
const GLYPH_GROUPS: [(&str, &str); 4] = [
    ("Uppercase", "ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
    ("Lowercase", "abcdefghijklmnopqrstuvwxyz"),
    ("Numbers", "0123456789"),
    (
        "Punctuation",
        ".,:;!?@#$%&*()[]{}+-=/\\_\u{2014}\u{2013}\u{201c}\u{201d}\u{2018}\u{2019}",
    ),
];

/// The preview's size, from 50% to 300% in 10% steps.
pub const MIN_SCALE: f32 = 0.5;
pub const MAX_SCALE: f32 = 3.;
const SCALE_STEP: f32 = 0.1;

/// `fontFormat`
pub fn format_name(mime: &str) -> &'static str {
    match mime {
        "font/collection" => "TrueType Collection",
        "font/otf" => "OpenType",
        "font/woff" => "Web Open Font Format",
        _ => "TrueType",
    }
}

/// A font file ready for the text system: its tables (a WOFF unpacked) and
/// the family names it may be registered under, most specific first.
pub struct LoadedFont {
    pub data: Vec<u8>,
    pub families: Vec<String>,
}

/// Reads a font file and the names of its first face.
pub fn load(path: &std::path::Path) -> Result<LoadedFont> {
    let bytes = std::fs::read(path)?;
    let data = if bytes.starts_with(b"wOFF") {
        unpack_woff(&bytes)?
    } else if bytes.starts_with(b"wOF2") {
        // TODO: unpack WOFF2 (Brotli and transformed tables) so these
        // fonts preview; Picto does not import WOFF2 today.
        bail!("WOFF2 fonts cannot be previewed yet.")
    } else {
        bytes
    };
    let face = ttf_parser::Face::parse(&data, 0).map_err(|_| anyhow!("Not a font file."))?;
    let mut families = Vec::new();
    for id in [
        ttf_parser::name_id::TYPOGRAPHIC_FAMILY,
        ttf_parser::name_id::FAMILY,
    ] {
        for name in face.names().into_iter().filter(|name| name.name_id == id) {
            if let Some(text) = name.to_string()
                && !families.contains(&text)
            {
                families.push(text);
            }
        }
    }
    if families.is_empty() {
        bail!("This font has no family name.");
    }
    Ok(LoadedFont { data, families })
}

/// The most a WOFF file unpacks to, and the most tables it may hold (a
/// real font has a few dozen).
const MAX_FONT: usize = 64 << 20;
const MAX_TABLES: usize = 1024;

/// A WOFF 1.0 file as the sfnt it wraps: the table directory rebuilt and
/// each table inflated where it was compressed. Every size is the file's
/// claim, so each is checked against the file and against [`MAX_FONT`].
pub fn unpack_woff(woff: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let read = |at: usize, width: usize| -> Result<u32> {
        let bytes = woff
            .get(at..at + width)
            .ok_or_else(|| anyhow!("The WOFF file is cut short."))?;
        Ok(bytes
            .iter()
            .fold(0, |value, byte| value << 8 | *byte as u32))
    };
    let flavor = read(4, 4)?;
    let count = read(12, 2)? as usize;
    if count == 0 || count > MAX_TABLES {
        bail!("This WOFF file has {count} tables.");
    }
    // The largest power of two not above the count, as the sfnt header's
    // binary search fields give it.
    let entry_selector = count.ilog2() as usize;
    let search_range = (1 << entry_selector) * 16;
    let mut out = Vec::new();
    for (value, width) in [
        (flavor, 4),
        (count as u32, 2),
        (search_range as u32, 2),
        (entry_selector as u32, 2),
        ((count * 16 - search_range) as u32, 2),
    ] {
        out.extend_from_slice(&value.to_be_bytes()[4 - width..]);
    }
    let mut tables = Vec::with_capacity(count);
    let mut total = 12 + 16 * count;
    for index in 0..count {
        let at = 44 + index * 20;
        let (tag, offset, compressed, length, checksum) = (
            read(at, 4)?,
            read(at + 4, 4)? as usize,
            read(at + 8, 4)? as usize,
            read(at + 12, 4)? as usize,
            read(at + 16, 4)?,
        );
        total = total.saturating_add(length.next_multiple_of(4));
        if total > MAX_FONT {
            bail!("This WOFF file unpacks to more than a font holds.");
        }
        let raw = offset
            .checked_add(compressed)
            .and_then(|end| woff.get(offset..end))
            .ok_or_else(|| anyhow!("The WOFF file is cut short."))?;
        let table = if compressed < length {
            let mut inflated = Vec::with_capacity(length);
            flate2::read::ZlibDecoder::new(raw)
                .take(length as u64 + 1)
                .read_to_end(&mut inflated)?;
            if inflated.len() != length {
                bail!("A table of this WOFF file is damaged.");
            }
            inflated
        } else {
            raw.to_vec()
        };
        tables.push((tag, checksum, table));
    }
    let mut offset = 12 + 16 * count;
    for (tag, checksum, table) in &tables {
        for value in [*tag, *checksum, offset as u32, table.len() as u32] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        offset += table.len().next_multiple_of(4);
    }
    for (_, _, table) in tables {
        let padded = table.len().next_multiple_of(4);
        out.extend_from_slice(&table);
        out.resize(out.len() + padded - table.len(), 0);
    }
    Ok(out)
}

/// The fonts added to the text system, by file, with the family each
/// answers to. GPUI cannot take a font out again, so a file viewed again is
/// not added again.
#[derive(Default)]
struct Registered(std::collections::HashMap<PathBuf, SharedString>);

impl Global for Registered {}

enum Status {
    Loading,
    Ready(SharedString),
    Failed,
}

pub struct FontViewer {
    name: SharedString,
    mime: String,
    status: Status,
    tab: Tab,
    paper: Paper,
    scale: f32,
    scroll: ScrollHandle,
    /// The preview's sample, editable in place (`contentEditable`): its
    /// heading, its paragraph and its alphabet.
    sample: [Entity<TextInput>; 3],
    /// The waterfall's lines, each editable on its own.
    waterfall: Vec<Entity<TextInput>>,
    _task: Task<()>,
}

impl FontViewer {
    pub fn new(path: PathBuf, name: SharedString, mime: String, cx: &mut Context<Self>) -> Self {
        let registered = cx
            .try_global::<Registered>()
            .and_then(|registered| registered.0.get(&path).cloned());
        let (status, task) = match registered {
            Some(family) => (Status::Ready(family), Task::ready(())),
            None => (Status::Loading, Self::load(path, cx)),
        };
        let field = |kind: Kind, text: String, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(kind, cx);
                input.set_text(text, cx);
                input
            })
        };
        let sample = [
            field(
                Kind::Area,
                t("The quick brown fox jumps over the lazy dog.").to_string(),
                cx,
            ),
            field(
                Kind::Area,
                t("Typography gives language a visible voice. Edit this sample to test the font with your own words, punctuation, and numbers.").to_string(),
                cx,
            ),
            field(
                Kind::Area,
                format!(
                    "{}\n{}\n0123456789",
                    t("ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
                    t("abcdefghijklmnopqrstuvwxyz")
                ),
                cx,
            ),
        ];
        let waterfall = WATERFALL_SIZES
            .iter()
            .map(|_| field(Kind::Form, t("Hamburgefontsiv 0123456789").to_string(), cx))
            .collect();
        let mut viewer = Self {
            name,
            mime,
            status,
            tab: Tab::Preview,
            paper: Paper::Auto,
            scale: 1.,
            scroll: ScrollHandle::new(),
            sample,
            waterfall,
            _task: task,
        };
        viewer.apply_faces(cx);
        viewer
    }

    /// The editable samples' faces: the font at the preview's size.
    fn apply_faces(&mut self, cx: &mut Context<Self>) {
        let Some(family) = self.family() else {
            return;
        };
        let scale = self.scale;
        let faces = sample_faces(&family, scale);
        for (input, face) in self.sample.iter().zip(faces) {
            input.update(cx, |input, cx| input.set_face(face, cx));
        }
        for (input, size) in self.waterfall.iter().zip(WATERFALL_SIZES) {
            let size = size * scale;
            let face = Face {
                family: family.clone(),
                size,
                line_height: size * 1.35,
                weight: FontWeight::NORMAL,
            };
            input.update(cx, |input, cx| input.set_face(face, cx));
        }
    }

    /// Reads the font off the drawing thread and adds it to the text
    /// system.
    fn load(path: PathBuf, cx: &mut Context<Self>) -> Task<()> {
        let file = path.clone();
        let read = cx
            .background_executor()
            .spawn(async move { super::guarded(|| load(&file)) });
        cx.spawn(async move |this, cx| {
            let loaded = read.await;
            let _ = this.update(cx, |this, cx| {
                this.status = match loaded.and_then(|font| register(font, cx)) {
                    Ok(family) => {
                        cx.default_global::<Registered>()
                            .0
                            .insert(path, family.clone());
                        Status::Ready(family)
                    }
                    Err(_) => Status::Failed,
                };
                this.apply_faces(cx);
                cx.notify();
            });
        })
    }

    /// Cmd+= and Cmd+−: the preview grows or shrinks by 10%.
    pub fn step_scale(&mut self, larger: bool, cx: &mut Context<Self>) {
        let step = if larger { SCALE_STEP } else { -SCALE_STEP };
        self.set_scale(self.scale + step, cx);
    }

    /// Cmd+0: the preview at its own size.
    pub fn reset_scale(&mut self, cx: &mut Context<Self>) {
        self.set_scale(1., cx);
    }

    fn set_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        self.scale = ((scale * 10.).round() / 10.).clamp(MIN_SCALE, MAX_SCALE);
        self.apply_faces(cx);
        cx.notify();
    }

    /// The family the preview is drawn in, once loaded.
    pub fn family(&self) -> Option<SharedString> {
        match &self.status {
            Status::Ready(family) => Some(family.clone()),
            _ => None,
        }
    }

    #[cfg(all(test, target_os = "macos"))]
    pub fn settled(&self) -> bool {
        !matches!(self.status, Status::Loading)
    }

    /// `--font-page-bg` and `--font-page-fg`.
    fn colors(&self, cx: &App) -> (Rgba, Rgba) {
        let app = theme::theme(cx);
        match self.paper {
            Paper::Auto => (app.background(), app.text()),
            Paper::Light => (
                rgb(theme::FONT_PAPER_LIGHT.0),
                rgb(theme::FONT_PAPER_LIGHT.1),
            ),
            Paper::Dark => (rgb(theme::FONT_PAPER_DARK.0), rgb(theme::FONT_PAPER_DARK.1)),
            Paper::Purple => (
                rgb(theme::FONT_PAPER_PURPLE.0),
                rgb(theme::FONT_PAPER_PURPLE.1),
            ),
            Paper::Yellow => (
                rgb(theme::FONT_PAPER_YELLOW.0),
                rgb(theme::FONT_PAPER_YELLOW.1),
            ),
        }
    }
}

/// The preview's sample in `family` at `scale`: the heading at 38 px on a
/// 1.25 line in the medium weight, the paragraphs at 18 px on a 1.6 line.
pub fn sample_faces(family: &SharedString, scale: f32) -> [Face; 3] {
    let heading = 38. * scale;
    let body = 18. * scale;
    let paragraph = Face {
        family: family.clone(),
        size: body,
        line_height: body * 1.6,
        weight: FontWeight::NORMAL,
    };
    [
        Face {
            family: family.clone(),
            size: heading,
            line_height: heading * 1.25,
            weight: FontWeight::MEDIUM,
        },
        paragraph.clone(),
        paragraph,
    ]
}

/// Adds the font to the text system and finds the family name it answers to.
fn register(font: LoadedFont, cx: &mut App) -> Result<SharedString> {
    let text_system = cx.text_system().clone();
    text_system.add_fonts(vec![Cow::Owned(font.data)])?;
    font.families
        .into_iter()
        .map(SharedString::from)
        .find(|family| {
            // A family the text system cannot load resolves to a fallback,
            // which is cached under another font.
            let id = text_system.resolve_font(&gpui::font(family.clone()));
            text_system
                .get_font_for_id(id)
                .is_some_and(|resolved| resolved.family == *family)
        })
        .ok_or_else(|| anyhow!("The text system did not take this font."))
}

/// `color-mix(in srgb, <fg> N%, transparent)`.
fn faded(color: Rgba, percent: f32) -> Rgba {
    Rgba {
        a: color.a * percent / 100.,
        ..color
    }
}

impl Render for FontViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = theme::theme(cx);
        let (paper, ink) = self.colors(cx);
        let family = self.family();
        let preview_font = family
            .clone()
            .unwrap_or_else(|| SharedString::from(theme::FONT_UI));
        let scale = self.scale;

        let header = div()
            .pt(px(4.))
            .pb(px(12.))
            .child(
                div()
                    .font_family(preview_font.clone())
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(px(32.))
                    .line_height(px(48.))
                    .truncate()
                    .child(self.name.clone()),
            )
            .child(
                div()
                    .text_size(px(theme::TEXT_XL))
                    .line_height(px(24.))
                    .text_color(faded(ink, 60.))
                    .child(format_name(&self.mime)),
            );

        let tabs = div()
            .id("font-tabs")
            .role(Role::TabList)
            .aria_label(t("Font preview mode"))
            .flex()
            .items_center()
            .gap(px(8.))
            .children(TABS.map(|(tab, label)| {
                let selected = self.tab == tab;
                div()
                    .id(label)
                    .role(Role::Tab)
                    .aria_label(t(label))
                    .aria_selected(selected)
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(theme::RADIUS_MD))
                    .flex()
                    .items_center()
                    .text_size(px(theme::TEXT_MD))
                    .line_height(px(32.))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .text_color(if selected { ink } else { faded(ink, 68.) })
                    .when(selected, |tab| tab.bg(faded(ink, 10.)))
                    .when(!selected, |tab| {
                        tab.hover(move |style| style.bg(faded(ink, 5.)).text_color(ink))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
                    .child(t(label))
            }));
        let papers = div()
            .id("font-papers")
            .role(Role::Group)
            .aria_label(t("Preview background"))
            .flex()
            .items_center()
            .gap(px(6.))
            .children(PAPERS.map(|choice| {
                let (fill, glyph): (Background, Rgba) = match choice {
                    Paper::Auto => (
                        linear_gradient(
                            135.,
                            linear_color_stop(rgb(theme::FONT_SWATCH_LIGHT.0), 0.5),
                            linear_color_stop(rgb(theme::FONT_SWATCH_DARK), 0.5),
                        ),
                        rgb(theme::FONT_SWATCH_AUTO_TEXT),
                    ),
                    Paper::Light => (
                        rgb(theme::FONT_SWATCH_LIGHT.0).into(),
                        rgb(theme::FONT_SWATCH_LIGHT.1),
                    ),
                    Paper::Dark => (
                        rgb(theme::FONT_SWATCH_DARK).into(),
                        rgba(theme::FONT_SWATCH_TEXT),
                    ),
                    Paper::Purple => (
                        rgb(theme::FONT_SWATCH_PURPLE).into(),
                        rgba(theme::FONT_SWATCH_TEXT),
                    ),
                    Paper::Yellow => (
                        rgb(theme::FONT_SWATCH_YELLOW.0).into(),
                        rgb(theme::FONT_SWATCH_YELLOW.1),
                    ),
                };
                let pressed = self.paper == choice;
                let ring = if pressed {
                    vec![
                        BoxShadow::new(px(0.), px(0.), paper.into()).spread_radius(px(2.)),
                        BoxShadow::new(px(0.), px(0.), faded(ink, 70.).into())
                            .spread_radius(px(3.)),
                    ]
                } else {
                    vec![
                        BoxShadow::new(px(0.), px(0.), rgba(theme::FONT_SWATCH_RING).into())
                            .spread_radius(px(1.))
                            .inset(),
                    ]
                };
                div()
                    .id(SharedString::from(format!("paper-{choice:?}")))
                    .role(Role::Button)
                    .aria_label(crate::tf!("{value0} preview", format!("{choice:?}").to_lowercase()))
                    .aria_toggled(crate::ui::a11y::toggled(pressed))
                    .size(px(20.))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(fill)
                    .shadow(ring)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(theme::TEXT_CAPTION))
                    .line_height(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(glyph)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.paper = choice;
                        cx.notify();
                    }))
                    .child(t("A"))
            }));
        let toolbar = div()
            .h(px(60.))
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(faded(ink, 8.))
            .child(tabs)
            .child(papers);

        let message = |text: &'static str| {
            div()
                .min_h(px(240.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(faded(ink, 60.))
                .child(text)
        };
        let content: AnyElement = match (&self.status, self.tab) {
            (_, Tab::Information) => {
                let row = |label: &'static str, value: SharedString| {
                    div()
                        .min_h(px(36.))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(faded(ink, 8.))
                        .child(
                            div()
                                .w(px(160.))
                                .flex_none()
                                .text_color(faded(ink, 60.))
                                .child(label),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(value))
                };
                div()
                    .text_size(px(theme::TEXT_MD))
                    .line_height(px((theme::TEXT_MD * 1.4).round()))
                    .child(row(t("File name"), self.name.clone()))
                    .child(row(t("Format"), format_name(&self.mime).into()))
                    .child(row(t("Preview engine"), "GPUI text system".into()))
                    .into_any_element()
            }
            (Status::Loading, _) => message(t("Loading font…")).into_any_element(),
            (Status::Failed, _) => message(en("This font cannot be previewed.")).into_any_element(),
            (Status::Ready(_), Tab::Preview) => {
                // `.article h2` and `.article p`, each edited in place.
                let [heading, paragraph, alphabet] = self.sample.clone();
                div()
                    .child(div().mb(px(24.)).child(heading))
                    .child(div().mb(px(20.)).child(paragraph))
                    .child(div().mb(px(20.)).child(alphabet))
                    .into_any_element()
            }
            // `.waterfall`: lines that run past the page are cut at its
            // edge.
            (Status::Ready(_), Tab::Waterfall) => div()
                .overflow_hidden()
                .flex()
                .flex_col()
                .children(
                    WATERFALL_SIZES
                        .iter()
                        .zip(&self.waterfall)
                        .map(|(size, input)| {
                            div()
                                .py(px(4.))
                                .h(px(size * scale * 1.35 + 8.))
                                .child(input.clone())
                        }),
                )
                .into_any_element(),
            (Status::Ready(family), Tab::Glyphs) => div()
                .children(GLYPH_GROUPS.map(|(label, glyphs)| {
                    div()
                        .mb(px(28.))
                        .child(
                            div()
                                .mb(px(10.))
                                .text_size(px(theme::TEXT_MD))
                                .line_height(px(20.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(faded(ink, 60.))
                                .child(t(label)),
                        )
                        .child(div().flex().flex_wrap().gap(px(1.)).children(
                            glyphs.chars().enumerate().map(|(index, glyph)| {
                                // TODO: production also scales the hovered
                                // glyph to 135%; GPUI cannot scale an element.
                                div()
                                    .id(SharedString::from(format!("{label}-{index}")))
                                    .size(px(50.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(3.))
                                    .bg(faded(ink, 5.))
                                    .font_family(family.clone())
                                    .text_size(px(24. * scale))
                                    .hover(move |style| {
                                        style.bg(paper).shadow(vec![
                                            BoxShadow::new(
                                                px(0.),
                                                px(8.),
                                                rgba(theme::FONT_GLYPH_SHADOW).into(),
                                            )
                                            .blur_radius(px(22.)),
                                            BoxShadow::new(px(0.), px(0.), faded(ink, 16.).into())
                                                .spread_radius(px(1.)),
                                        ])
                                    })
                                    .child(glyph.to_string())
                            }),
                        ))
                }))
                .into_any_element(),
        };

        // `.zoom`: a glass bar fixed at the bottom centre while a mode
        // that uses the size is shown.
        let zoom = (self.tab != Tab::Information).then(|| {
            let step =
                |id: &'static str, icon: &'static str, larger: bool, cx: &mut Context<Self>| {
                    ui::icon_button(id, icon)
                        .name(if larger {
                            t("Increase font preview size")
                        } else {
                            t("Decrease font preview size")
                        })
                        .glyph(14.)
                        .on_click(cx.listener(move |this, _, _, cx| this.step_scale(larger, cx)))
                };
            div()
                .absolute()
                .bottom(px(12.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    ui::glass_panel("font-zoom", theme::RADIUS_MD, cx)
                        .occlude()
                        .w(px(164.))
                        .h(px(32.))
                        // `.zoom`'s 4px padding inside its 1px glass border.
                        .px(px(5.))
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .child(step("font-smaller", "minus", false, cx))
                        .child(
                            div().flex_1().flex().justify_center().child(
                                ui::slider(
                                    "font-size",
                                    (self.scale * 100.).round() as f64,
                                    50.,
                                    300.,
                                    1.,
                                )
                                .name(t("Font preview size"))
                                .width(96.)
                                .on_change({
                                    let entity = cx.entity().downgrade();
                                    move |value, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            this.set_scale(value as f32 / 100., cx)
                                        });
                                    }
                                }),
                            ),
                        )
                        .child(step("font-larger", "plus", true, cx)),
                )
        });

        div()
            .absolute()
            .inset_0()
            .bg(app.background())
            .text_color(app.text())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .child(
                div()
                    .id("font-scroll")
                    .track_scroll(&self.scroll)
                    .overflow_y_scroll()
                    .size_full()
                    .child(
                        // `.page`: `min(900px, 100% - 96px)` wide, centred in
                        // the view less its scrollbar's gutter
                        // (`scrollbar-gutter: stable`), 24px from the top.
                        div()
                            .w_full()
                            .min_h_full()
                            .pl(px(48.))
                            .pr(px(48. + ui::scrollbar::GUTTER))
                            .py(px(24.))
                            .child(
                                div()
                                    .mx_auto()
                                    .w_full()
                                    .max_w(px(900.))
                                    .min_h_full()
                                    .px(px(48.))
                                    .pb(px(72.))
                                    .bg(paper)
                                    .text_color(ink)
                                    .child(header)
                                    .child(toolbar)
                                    .child(div().pt(px(20.)).child(content)),
                            ),
                    ),
            )
            .child(ui::scrollbar("font-scrollbar", &self.scroll, window, cx))
            .children(zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::{format_name, load, sample_faces, unpack_woff};
    use gpui::{FontWeight, SharedString};

    #[test]
    fn the_sample_is_edited_in_the_font_at_the_preview_size() {
        let family = SharedString::from("Fira Mono");
        let [heading, paragraph, alphabet] = sample_faces(&family, 1.5);
        // `.article h2`: 38px on a 1.25 line, medium; `.article p`: 18px on
        // a 1.6 line; both times the preview's scale.
        assert_eq!((heading.size, heading.line_height), (57., 71.25));
        assert_eq!(heading.weight, FontWeight::MEDIUM);
        assert_eq!((paragraph.size, paragraph.line_height), (27., 43.2));
        assert_eq!(paragraph, alphabet);
        assert_eq!(heading.family, family);
    }

    #[test]
    fn formats_are_named_as_production_names_them() {
        assert_eq!(format_name("font/collection"), "TrueType Collection");
        assert_eq!(format_name("font/otf"), "OpenType");
        assert_eq!(format_name("font/woff"), "Web Open Font Format");
        assert_eq!(format_name("font/ttf"), "TrueType");
    }

    #[test]
    fn the_bundled_font_names_its_family() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/FiraMono.ttf");
        let font = load(&path).unwrap();
        assert!(font.families.iter().any(|family| family == "Fira Mono"));
    }

    /// A WOFF header claiming `count` tables, followed by `entries`.
    fn woff(count: u16, entries: &[[u32; 5]], data: &[u8]) -> Vec<u8> {
        let mut woff = b"wOFF".to_vec();
        woff.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        woff.extend_from_slice(&[0; 4]);
        woff.extend_from_slice(&count.to_be_bytes());
        woff.resize(44, 0);
        for entry in entries {
            for value in entry {
                woff.extend_from_slice(&value.to_be_bytes());
            }
        }
        woff.extend_from_slice(data);
        woff
    }

    #[test]
    fn a_damaged_woff_is_refused_without_overflow_or_a_vast_allocation() {
        use std::io::Write;
        // More tables than the sfnt header can count, with or without the
        // entries behind them.
        assert!(unpack_woff(&woff(u16::MAX, &[], &[])).is_err());
        assert!(unpack_woff(&woff(4096, &[[0; 5]; 4096], &[])).is_err());
        assert!(unpack_woff(&woff(0, &[], &[])).is_err());
        // A table that claims 4 GB, and one whose place runs past the end
        // of the file (and of the address space).
        let huge = [u32::from_be_bytes(*b"glyf"), 64, 4, u32::MAX, 0];
        assert!(unpack_woff(&woff(1, &[huge], &[0; 4])).is_err());
        let past = [
            u32::from_be_bytes(*b"glyf"),
            u32::MAX,
            u32::MAX,
            u32::MAX,
            0,
        ];
        assert!(unpack_woff(&woff(1, &[past], &[])).is_err());
        // A compressed table that inflates to more than it claims.
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&vec![0; 1 << 20]).unwrap();
        let bomb = encoder.finish().unwrap();
        let entry = [
            u32::from_be_bytes(*b"glyf"),
            64,
            bomb.len() as u32,
            1 << 12,
            0,
        ];
        assert!(unpack_woff(&woff(1, &[entry], &bomb)).is_err());
        // Cut short anywhere.
        let whole = woff(
            1,
            &[[u32::from_be_bytes(*b"head"), 64, 4, 4, 0]],
            &[1, 2, 3, 4],
        );
        assert!(unpack_woff(&whole).is_ok());
        for end in 0..whole.len() {
            assert!(unpack_woff(&whole[..end]).is_err(), "cut at {end}");
        }
    }
}
