//! A text document's first page as a picture, for its thumbnail. Production
//! renders the document in its viewer in a hidden 1,328 × 1,200 window and
//! captures the page (`documentThumbnailService.mjs`,
//! `entrypoints/documentThumbnail.tsx`). GPUI cannot read back what a window
//! drew, so the page is laid out here as [`super::reader`] lays it out (the
//! same widths, padding, sizes, line heights, margins and colours) with
//! cosmic-text and drawn into an image, at the hidden window's two pixels a
//! point. Only what the window would show is drawn: the page from its top,
//! at most as tall as the window below its padding.
use super::reader::{self, PageStyle};
use super::rich::{Block, Inline, Marks};
use crate::theme;
use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight,
};
use image::{Rgba, RgbaImage};
use std::sync::Mutex;

/// The capture's two pixels a point.
const SCALE: f32 = 2.;
/// The hidden window's height, less the viewer's padding above the page.
const MAX_HEIGHT: f32 = 1_200. - super::shell::VIEWPORT_PADDING;
/// The available height the page's minimum is taken from.
const AVAILABLE_HEIGHT: f32 = 1_200. - 2. * super::shell::VIEWPORT_PADDING;
/// Inline code's metadata on its glyphs, for its tint.
const CODE: usize = 1;

/// The fonts, loaded once: the system's and the two the app bundles.
static FONTS: Mutex<Option<(FontSystem, SwashCache)>> = Mutex::new(None);

/// The UI face (`--font-family-ui`) as the system names it.
fn ui_family() -> Family<'static> {
    if cfg!(target_os = "macos") {
        // i18n: a font's name
        Family::Name("System Font")
    } else if cfg!(windows) {
        // i18n: a font's name
        Family::Name("Segoe UI")
    } else {
        Family::SansSerif
    }
}

/// The extra advance, in ems, that brings the system font's default
/// instance to the width Chromium draws it at `size` with, where it picks
/// the font's optical size for the text's size: none on a heading's large
/// sizes, about 6% of an em at the page's 14 px. Measured against
/// production's thumbnails; elsewhere the system's UI face is static.
pub fn tracking(size: f32) -> f32 {
    if cfg!(target_os = "macos") {
        (0.062 - (size - 14.) * 0.00364).clamp(0., 0.062)
    } else {
        0.
    }
}

fn family(style: PageStyle) -> Family<'static> {
    match style {
        PageStyle::Text => ui_family(),
        PageStyle::Book => Family::Name(theme::FONT_BOOK),
    }
}

fn mono() -> Family<'static> {
    Family::Name(theme::FONT_MONO)
}

fn color(rgb: u32) -> Color {
    Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// The page's height in points: its content within its padding, at least
/// its minimum, and what the window shows of it.
pub fn page_height(content: f32, style: PageStyle) -> f32 {
    let (vertical, _) = style.padding();
    let minimum = match style {
        PageStyle::Text => AVAILABLE_HEIGHT.min(1_056.),
        PageStyle::Book => AVAILABLE_HEIGHT,
    };
    (content + 2. * vertical).max(minimum).min(MAX_HEIGHT)
}

/// Draws the page of `blocks` in `style`.
pub fn draw(blocks: &[Block], style: PageStyle) -> RgbaImage {
    let mut fonts = FONTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (system, cache) = fonts.get_or_insert_with(|| {
        let mut system = FontSystem::new();
        let db = system.db_mut();
        db.load_font_data(include_bytes!("../../../assets/FiraMono.ttf").to_vec());
        db.load_font_data(include_bytes!("../../../assets/Roboto.ttf").to_vec());
        (system, SwashCache::new())
    });
    let mut page = Page {
        system,
        cache,
        style,
        commands: Vec::new(),
    };
    let (vertical, horizontal) = style.padding();
    let width = style.max_width() - 2. * horizontal;
    let content = page.blocks(blocks, horizontal, vertical, width);
    let height = page_height(content - vertical, style);
    let mut picture = RgbaImage::from_pixel(
        (style.max_width() * SCALE).round() as u32,
        (height * SCALE).round() as u32,
        Rgba(paper()),
    );
    for command in page.commands.drain(..) {
        command.paint(&mut picture, page.system, page.cache);
    }
    picture
}

fn paper() -> [u8; 4] {
    let paper = theme::DOCUMENT_PAPER;
    [(paper >> 16) as u8, (paper >> 8) as u8, paper as u8, 255]
}

/// What is drawn, in order, once the page's height is known.
enum Command {
    Fill {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: u32,
    },
    Text {
        buffer: Buffer,
        x: f32,
        y: f32,
        color: u32,
    },
    Picture {
        rgba: RgbaImage,
        x: f32,
        y: f32,
    },
}

impl Command {
    fn paint(self, picture: &mut RgbaImage, system: &mut FontSystem, cache: &mut SwashCache) {
        match self {
            Command::Fill {
                x,
                y,
                width,
                height,
                color,
            } => fill(
                picture,
                (x * SCALE).round() as i32,
                (y * SCALE).round() as i32,
                (width * SCALE).round() as u32,
                (height * SCALE).round() as u32,
                rgba(color),
            ),
            Command::Text {
                mut buffer,
                x,
                y,
                color: base,
            } => {
                let (left, top) = ((x * SCALE).round() as i32, (y * SCALE).round() as i32);
                // Inline code's tint, behind its glyphs, a line high.
                let mut tints = Vec::new();
                for run in buffer.layout_runs() {
                    for glyph in run.glyphs.iter().filter(|glyph| glyph.metadata == CODE) {
                        tints.push((glyph.x, run.line_top, glyph.w, run.line_height));
                    }
                }
                for (gx, gy, gw, gh) in tints {
                    fill(
                        picture,
                        left + gx.floor() as i32,
                        top + gy.round() as i32,
                        gw.ceil() as u32 + 1,
                        gh.round() as u32,
                        rgba(theme::DOCUMENT_CODE),
                    );
                }
                buffer.draw(system, cache, color(base), |gx, gy, w, h, glyph| {
                    let [r, g, b, a] = glyph.as_rgba();
                    fill(picture, left + gx, top + gy, w, h, [r, g, b, a]);
                });
            }
            Command::Picture { rgba, x, y } => {
                image::imageops::overlay(
                    picture,
                    &rgba,
                    (x * SCALE).round() as i64,
                    (y * SCALE).round() as i64,
                );
            }
        }
    }
}

fn rgba(color: u32) -> [u8; 4] {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8, 255]
}

/// Blends `color` over the rectangle, clipped to the picture.
fn fill(picture: &mut RgbaImage, x: i32, y: i32, width: u32, height: u32, color: [u8; 4]) {
    let alpha = color[3] as u32;
    if alpha == 0 {
        return;
    }
    let (right, bottom) = (picture.width() as i32, picture.height() as i32);
    for py in y.max(0)..(y + height as i32).min(bottom) {
        for px in x.max(0)..(x + width as i32).min(right) {
            let pixel = picture.get_pixel_mut(px as u32, py as u32);
            for channel in 0..3 {
                let over = color[channel] as u32 * alpha + pixel[channel] as u32 * (255 - alpha);
                pixel[channel] = (over / 255) as u8;
            }
        }
    }
}

struct Page<'a> {
    system: &'a mut FontSystem,
    cache: &'a mut SwashCache,
    style: PageStyle,
    commands: Vec<Command>,
}

impl Page<'_> {
    fn font_size(&self) -> f32 {
        self.style.font_size()
    }

    /// Lays out text of `size` on lines of `line` points, `width` wide,
    /// at (`x`, `y`), and hands back its height and the width of its
    /// longest line.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        spans: &[(&str, Attrs)],
        size: f32,
        line: f32,
        x: f32,
        y: f32,
        width: Option<f32>,
        color: u32,
    ) -> (f32, f32) {
        let mut buffer = Buffer::new(self.system, Metrics::new(size * SCALE, line * SCALE));
        buffer.set_size(width.map(|width| width * SCALE), None);
        let defaults = spans
            .first()
            .map(|(_, attrs)| attrs.clone())
            .unwrap_or_else(|| Attrs::new().family(family(self.style)));
        // The UI face's runs take its tracking at this size.
        let ui = ui_family();
        let spans = spans.iter().map(|(text, attrs)| {
            let attrs = if attrs.family == ui {
                attrs.clone().letter_spacing(tracking(size))
            } else {
                attrs.clone()
            };
            (*text, attrs)
        });
        buffer.set_rich_text(spans, &defaults, Shaping::Advanced, None);
        buffer.shape_until_scroll(self.system, false);
        let (mut lines, mut widest) = (0, 0f32);
        for run in buffer.layout_runs() {
            lines += 1;
            widest = widest.max(run.line_w);
        }
        let height = lines.max(1) as f32 * line;
        self.commands.push(Command::Text {
            buffer,
            x,
            y,
            color,
        });
        (height, widest / SCALE)
    }

    /// An inline paragraph, `width` wide, and its height.
    #[allow(clippy::too_many_arguments)]
    fn inline(
        &mut self,
        inline: &Inline,
        size: f32,
        line: f32,
        x: f32,
        y: f32,
        width: f32,
        bold: bool,
        color: u32,
    ) -> f32 {
        let base = Attrs::new()
            .family(family(self.style))
            .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
            .color(self::color(color));
        let spans = spans_of(inline, base, self.style);
        self.text(&spans, size, line, x, y, Some(width), color).0
    }

    /// Lays out `blocks` from `y` and hands back where they end.
    fn blocks(&mut self, blocks: &[Block], x: f32, mut y: f32, width: f32) -> f32 {
        let style = self.style;
        if style == PageStyle::Text && reader::plain_text_page(blocks) {
            let size = theme::TEXT_MD;
            let line = (size * 1.6).round();
            for block in blocks {
                if y > MAX_HEIGHT {
                    break;
                }
                let Block::Preformatted(text) = block else {
                    continue;
                };
                let text = reader::expand_tabs(text);
                let attrs = Attrs::new().family(mono()).color(color(style.color()));
                y += self
                    .text(
                        &[(text.as_str(), attrs)],
                        size,
                        line,
                        x,
                        y,
                        Some(width),
                        style.color(),
                    )
                    .0;
            }
            return y;
        }
        let font_size = self.font_size();
        let spaces = reader::spacing(blocks, font_size, style);
        for (block, (top, bottom)) in blocks.iter().zip(spaces) {
            if y > MAX_HEIGHT {
                break;
            }
            y += top;
            y = self.block(block, x, y, width, style.color());
            y += bottom;
        }
        y
    }

    fn block(&mut self, block: &Block, x: f32, y: f32, width: f32, text_color: u32) -> f32 {
        let style = self.style;
        let font_size = self.font_size();
        let line = (font_size * 1.65).round();
        match block {
            Block::Heading(level, inline) => {
                let size = reader::heading_size(*level, font_size);
                let ratio = match style {
                    PageStyle::Text => 1.25,
                    PageStyle::Book => 1.65,
                };
                y + self.inline(
                    inline,
                    size,
                    (size * ratio).round(),
                    x,
                    y,
                    width,
                    true,
                    text_color,
                )
            }
            Block::Paragraph(inline) => {
                y + self.inline(inline, font_size, line, x, y, width, false, text_color)
            }
            Block::Preformatted(text) => {
                let text = reader::expand_tabs(text);
                let attrs = Attrs::new().family(mono()).color(color(text_color));
                let (padding_x, padding_y) = match style {
                    PageStyle::Text => (0.3 * font_size, 0.12 * font_size),
                    PageStyle::Book => (0., 0.),
                };
                let at = self.commands.len();
                let (height, widest) = self.text(
                    &[(text.as_str(), attrs)],
                    font_size,
                    line,
                    x + padding_x,
                    y + padding_y,
                    Some(width - 2. * padding_x),
                    text_color,
                );
                if style == PageStyle::Text {
                    // The tint as wide as the text, behind it.
                    self.commands.insert(
                        at,
                        Command::Fill {
                            x,
                            y,
                            width: widest + 2. * padding_x,
                            height: height + 2. * padding_y,
                            color: theme::DOCUMENT_CODE,
                        },
                    );
                }
                y + height + 2. * padding_y
            }
            Block::Quote(inner) => match style {
                PageStyle::Text => {
                    let at = self.commands.len();
                    let end = self.quote(inner, x + 19., y, width - 19.);
                    self.commands.insert(
                        at,
                        Command::Fill {
                            x,
                            y,
                            width: 3.,
                            height: end - y,
                            color: theme::DOCUMENT_QUOTE_RULE,
                        },
                    );
                    end
                }
                PageStyle::Book => self.quote(inner, x, y, width),
            },
            Block::List { start, items } => {
                let mut y = y;
                for (index, item) in items.iter().enumerate() {
                    let marker = match start {
                        Some(start) => format!("{}.", start + index as u64),
                        None => "•".to_owned(),
                    };
                    // The marker hangs left of the item, 6 px from it.
                    let attrs = Attrs::new().family(family(style)).color(color(text_color));
                    let marker_width = measure(self.system, &marker, attrs.clone(), font_size);
                    self.text(
                        &[(marker.as_str(), attrs)],
                        font_size,
                        line,
                        x - 6. - marker_width,
                        y,
                        None,
                        text_color,
                    );
                    for block in item {
                        y = self.block(block, x, y, width, text_color);
                    }
                }
                y
            }
            Block::Table(rows) => {
                let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
                let mut left = x + 2.;
                for column in 0..columns {
                    let mut top = y + 2.;
                    let mut widest = 0f32;
                    for row in rows {
                        if let Some(cell) = row.get(column) {
                            let base = Attrs::new().family(family(style)).color(color(text_color));
                            let spans = spans_of(cell, base, style);
                            let (_, cell_width) =
                                self.text(&spans, font_size, line, left, top, None, text_color);
                            widest = widest.max(cell_width);
                        }
                        top += line + 2.;
                    }
                    left += widest + 2.;
                }
                y + 2. + rows.len() as f32 * (line + 2.)
            }
            Block::Rule => {
                self.commands.push(Command::Fill {
                    x,
                    y,
                    width,
                    height: 1.,
                    color: theme::DOCUMENT_QUOTE_RULE,
                });
                y + 2.
            }
            Block::Picture(picture) => {
                let (natural_width, natural_height) = picture.size;
                let scale = (width / natural_width).min(1.);
                let (shown_width, shown_height) = (natural_width * scale, natural_height * scale);
                if let Some(rgba) = picture_pixels(&picture.image, shown_width, shown_height) {
                    self.commands.push(Command::Picture { rgba, x, y });
                }
                y + shown_height
            }
        }
    }

    fn quote(&mut self, inner: &[Block], x: f32, y: f32, width: f32) -> f32 {
        let color = match self.style {
            PageStyle::Text => theme::DOCUMENT_QUOTE,
            PageStyle::Book => self.style.color(),
        };
        let mut y = y;
        for block in inner {
            y = self.block(block, x, y, width, color);
        }
        y
    }
}

fn spans_of<'a>(
    inline: &'a Inline,
    base: Attrs<'static>,
    style: PageStyle,
) -> Vec<(&'a str, Attrs<'static>)> {
    let mut spans = Vec::new();
    let mut at = 0;
    for (range, marks) in &inline.marks {
        if range.start > at {
            spans.push((&inline.text[at..range.start], base.clone()));
        }
        spans.push((
            &inline.text[range.clone()],
            marked(base.clone(), *marks, style),
        ));
        at = range.end;
    }
    if at < inline.text.len() || spans.is_empty() {
        spans.push((&inline.text[at..], base));
    }
    spans
}

/// A run's marks as text attributes (`reader::highlight`).
fn marked(mut attrs: Attrs<'static>, marks: Marks, style: PageStyle) -> Attrs<'static> {
    if marks.bold {
        attrs = attrs.weight(Weight::BOLD);
    }
    if marks.italic {
        attrs = attrs.style(Style::Italic);
    }
    if marks.strike {
        attrs = attrs.strikethrough();
    }
    if marks.link {
        attrs = attrs
            .color(color(theme::DOCUMENT_LINK))
            .underline(cosmic_text::UnderlineStyle::Single);
    }
    if marks.code {
        attrs = attrs.family(mono());
        if style == PageStyle::Text {
            attrs = attrs.metadata(CODE);
        }
    }
    attrs
}

/// The width of one line of `text`, in points.
fn measure(system: &mut FontSystem, text: &str, attrs: Attrs, size: f32) -> f32 {
    let mut buffer = Buffer::new(system, Metrics::new(size * SCALE, size * SCALE));
    buffer.set_size(None, None);
    buffer.set_rich_text([(text, attrs.clone())], &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);
    buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0., f32::max)
        / SCALE
}

/// A chapter's picture at its shown size, in pixels, from GPUI's BGRA frame.
fn picture_pixels(image: &gpui::RenderImage, width: f32, height: f32) -> Option<RgbaImage> {
    let size = image.size(0);
    let (natural_width, natural_height) = (size.width.0 as u32, size.height.0 as u32);
    let mut bytes = image.as_bytes(0)?.to_vec();
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let natural = RgbaImage::from_raw(natural_width, natural_height, bytes)?;
    Some(image::imageops::resize(
        &natural,
        ((width * SCALE).round() as u32).max(1),
        ((height * SCALE).round() as u32).max(1),
        image::imageops::FilterType::Triangle,
    ))
}

#[cfg(test)]
mod tests {
    use super::{PageStyle, draw, page_height};
    use crate::viewer::document::rich;

    #[test]
    fn the_page_is_the_window_s_share_of_it() {
        // A short text page keeps its 1,056 points; a long one is cut where
        // the hidden window ends, 24 points of padding below its top.
        assert_eq!(page_height(100., PageStyle::Text), 1_056.);
        assert_eq!(page_height(5_000., PageStyle::Text), 1_176.);
        // A book page is at least the window's height inside its padding.
        assert_eq!(page_height(100., PageStyle::Book), 1_152.);
    }

    #[test]
    fn draws_text_on_the_paper() {
        let picture = draw(
            &rich::markdown("# Title\n\nSome **bold** text and `code`.\n\n- one\n- two\n"),
            PageStyle::Text,
        );
        assert_eq!(picture.dimensions(), (1_632, 2_112));
        // Paper at the corner, ink where the title is.
        assert_eq!(picture.get_pixel(0, 0).0, [255, 255, 255, 255]);
        let inked = (152..400u32)
            .flat_map(|y| (144..600u32).map(move |x| (x, y)))
            .filter(|(x, y)| picture.get_pixel(*x, *y).0[0] < 128)
            .count();
        assert!(inked > 500, "{inked} dark pixels in the title");
    }

    #[test]
    #[ignore = "draws PICTO_GPUI_MEDIA (of type PICTO_GPUI_MEDIA_MIME) to PICTO_GPUI_MATRIX_OUT"]
    fn draws_a_real_document() {
        let path = std::path::PathBuf::from(std::env::var_os("PICTO_GPUI_MEDIA").unwrap());
        let out = std::path::PathBuf::from(std::env::var_os("PICTO_GPUI_MATRIX_OUT").unwrap());
        let mime = std::env::var("PICTO_GPUI_MEDIA_MIME").unwrap();
        crate::grid::rendered::render(&mime, &path, &out).unwrap();
    }
}
