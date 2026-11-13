//! Text documents and EPUB books on a white page in the document shell
//! (`TextDocumentViewer.tsx`, and the EPUB half of
//! `ArchiveDocumentViewer.tsx`). The file is read and parsed off the drawing
//! thread; the blocks are laid out with GPUI's text. A book shows one
//! chapter to a page.
use super::kind::RendererKind;
use super::rich::{self, Block, Inline, Marks};
use super::shell::{self, VIEWPORT_PADDING};
use crate::theme;
use anyhow::{Result, anyhow};
use gpui::*;
use std::path::PathBuf;
use std::sync::Arc;

/// Text files are read up to this size; the rest is left out and said so.
pub const TEXT_LIMIT: usize = 4 << 20;

/// How the page looks: the text page or the book page.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PageStyle {
    /// `.page`: 816px wide at most, at least 1056px (or the viewport) tall,
    /// 72px by 76px of padding, 14px UI text on a 1.65 line.
    Text,
    /// `.bookPage`: 760px wide at most, at least the viewport tall, 56px by
    /// 64px of padding, 16px Georgia on a 1.65 line.
    Book,
}

impl PageStyle {
    fn max_width(self) -> f32 {
        match self {
            PageStyle::Text => 816.,
            PageStyle::Book => 760.,
        }
    }
    fn padding(self) -> (f32, f32) {
        match self {
            PageStyle::Text => (72., 76.),
            PageStyle::Book => (56., 64.),
        }
    }
    fn font_size(self) -> f32 {
        match self {
            PageStyle::Text => 14.,
            PageStyle::Book => 16.,
        }
    }
    fn font(self) -> &'static str {
        match self {
            PageStyle::Text => theme::FONT_UI,
            PageStyle::Book => theme::FONT_BOOK,
        }
    }
    fn color(self) -> u32 {
        match self {
            PageStyle::Text => theme::DOCUMENT_TEXT,
            PageStyle::Book => theme::BOOK_TEXT,
        }
    }
}

/// Reads a text document into blocks by its media type
/// (`TextDocumentViewer.tsx`).
pub fn load_text(path: &std::path::Path, mime: &str) -> Result<(Vec<Block>, bool)> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| anyhow!("Could not open this document."))?;
    let mut bytes = Vec::new();
    file.take(TEXT_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > TEXT_LIMIT;
    bytes.truncate(TEXT_LIMIT);
    let text = String::from_utf8_lossy(&bytes);
    let blocks = match mime {
        "text/markdown" => rich::markdown(&text),
        "application/json" if !truncated => rich::json(&text),
        "application/rtf" | "text/rtf" => rich::rtf(&text),
        _ => rich::plain_text(&text),
    };
    Ok((blocks, truncated))
}

pub struct ReaderViewer {
    path: PathBuf,
    mime: String,
    style: PageStyle,
    /// A book's chapters; empty for a text document.
    chapters: Vec<String>,
    /// 0-based chapter.
    page: usize,
    blocks: Option<Arc<Vec<Block>>>,
    truncated: bool,
    error: Option<SharedString>,
    task: Option<Task<()>>,
    scroll: ScrollHandle,
    /// The viewport's content box.
    available: (f32, f32),
}

impl ReaderViewer {
    pub fn new(kind: RendererKind, path: PathBuf, mime: String, cx: &mut Context<Self>) -> Self {
        let style = if kind == RendererKind::Epub {
            PageStyle::Book
        } else {
            PageStyle::Text
        };
        let mut this = Self {
            path,
            mime,
            style,
            chapters: Vec::new(),
            page: 0,
            blocks: None,
            truncated: false,
            error: None,
            task: None,
            scroll: ScrollHandle::new(),
            available: (0., 0.),
        };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let mime = self.mime.clone();
        let style = self.style;
        let chapters = self.chapters.clone();
        let page = self.page;
        let work = cx.background_executor().spawn(async move {
            if style == PageStyle::Book {
                let chapters = if chapters.is_empty() {
                    super::epub::chapters(&path)?
                } else {
                    chapters
                };
                let chapter = chapters
                    .get(page)
                    .ok_or_else(|| anyhow!("No page {}", page + 1))?;
                let blocks = super::epub::chapter(&path, chapter)?;
                Ok::<_, anyhow::Error>((chapters, blocks, false))
            } else {
                let (blocks, truncated) = load_text(&path, &mime)?;
                Ok((Vec::new(), blocks, truncated))
            }
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((chapters, blocks, truncated)) => {
                        if this.page != page {
                            return;
                        }
                        this.chapters = chapters;
                        this.blocks = Some(Arc::new(blocks));
                        this.truncated = truncated;
                        this.error = None;
                        this.scroll.set_offset(point(px(0.), px(0.)));
                    }
                    Err(error) => this.error = Some(error.to_string().into()),
                }
                cx.notify();
            });
        }));
    }

    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        if self.page > 0 {
            self.page -= 1;
            self.load(cx);
        }
    }

    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        if self.page + 1 < self.chapters.len() {
            self.page += 1;
            self.load(cx);
        }
    }

    /// Whether the document has been laid out, for checks that wait on it.
    #[cfg(test)]
    pub fn loaded(&self) -> bool {
        self.blocks.is_some() || self.error.is_some()
    }

    fn count(&self) -> usize {
        if self.style == PageStyle::Book {
            self.chapters.len()
        } else {
            1
        }
    }
}

impl Render for ReaderViewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.style;
        let page = self.blocks.as_ref().map(|blocks| {
            let (vertical, horizontal) = style.padding();
            // `width: min(<max>, 100%)`
            let width = super::geometry::bound_page_width(
                self.available.0,
                (style.max_width(), 1.),
                style.max_width(),
            )
            .map_or(0., |size| size.0);
            let min_height = match style {
                PageStyle::Text => self.available.1.min(1056.),
                PageStyle::Book => self.available.1,
            };
            let mut children = blocks_view(blocks, style);
            if self.truncated {
                children.push(
                    div()
                        .mt(px(style.font_size()))
                        .text_color(rgb(theme::DOCUMENT_QUOTE))
                        .italic()
                        .child(format!(
                            "Only the first {} MB of this file is shown.",
                            TEXT_LIMIT >> 20
                        ))
                        .into_any_element(),
                );
            }
            div()
                .flex_none()
                .w(px(width))
                .min_h(px(min_height))
                .py(px(vertical))
                .px(px(horizontal))
                .flex()
                .flex_col()
                .bg(rgb(theme::DOCUMENT_PAPER))
                .shadow(if style == PageStyle::Text {
                    theme::page_shadow()
                } else {
                    theme::sheet_shadow()
                })
                .font_family(style.font())
                .text_size(px(style.font_size()))
                .line_height(px((style.font_size() * 1.65).round()))
                .text_color(rgb(style.color()))
                .children(children)
        });
        let entity = cx.entity().downgrade();
        let (previous, next) = (entity.clone(), entity.clone());
        shell::shell(&self.scroll, self.page + 1, self.count())
            .error(self.error.clone())
            .content(page)
            .on_previous(move |_, cx| {
                let _ = previous.update(cx, |this, cx| this.previous_page(cx));
            })
            .on_next(move |_, cx| {
                let _ = next.update(cx, |this, cx| this.next_page(cx));
            })
            .on_measure(move |bounds, cx| {
                let available = (
                    (f32::from(bounds.size.width) - 2. * VIEWPORT_PADDING).max(0.),
                    (f32::from(bounds.size.height) - 2. * VIEWPORT_PADDING).max(0.),
                );
                let _ = entity.update(cx, |this, cx| {
                    if this.available != available {
                        this.available = available;
                        cx.notify();
                    }
                });
            })
    }
}

/// The vertical margins a block takes, top and bottom, in pixels, by the
/// browser's defaults and the page's rules.
pub fn margins(block: &Block, font_size: f32) -> (f32, f32) {
    match block {
        // `margin: 1.4em 0 0.55em` in the heading's own size.
        Block::Heading(level, _) => {
            let size = heading_size(*level, font_size);
            (1.4 * size, 0.55 * size)
        }
        Block::Paragraph(_)
        | Block::Preformatted(_)
        | Block::Quote(_)
        | Block::List { .. }
        | Block::Table(_) => (font_size, font_size),
        Block::Rule => (0.5 * font_size, 0.5 * font_size),
        Block::Picture(_) => (0., 0.),
    }
}

/// The browser's heading sizes: 2, 1.5, 1.17, 1, 0.83 and 0.67 em.
pub fn heading_size(level: u8, font_size: f32) -> f32 {
    font_size
        * match level {
            1 => 2.,
            2 => 1.5,
            3 => 1.17,
            4 => 1.,
            5 => 0.83,
            _ => 0.67,
        }
}

/// Spacing between blocks as collapsed margins: the larger of the two
/// margins that meet. The page's first heading of level 1 or 2 has no top
/// margin (`h1:first-child`).
pub fn spacing(blocks: &[Block], font_size: f32) -> Vec<(f32, f32)> {
    blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let (top, bottom) = margins(block, font_size);
            let above = match index {
                0 if matches!(block, Block::Heading(1 | 2, _)) => 0.,
                0 => top,
                _ => top.max(margins(&blocks[index - 1], font_size).1),
            };
            let below = if index + 1 == blocks.len() {
                bottom
            } else {
                0.
            };
            (above, below)
        })
        .collect()
}

/// The page's single `<pre>` (`.plainText`): no margins, 13px monospace on
/// a 1.6 line, wrapping.
fn plain_text_page(blocks: &[Block]) -> bool {
    !blocks.is_empty()
        && blocks
            .iter()
            .all(|block| matches!(block, Block::Preformatted(_)))
}

fn blocks_view(blocks: &[Block], style: PageStyle) -> Vec<AnyElement> {
    if style == PageStyle::Text && plain_text_page(blocks) {
        return blocks
            .iter()
            .map(|block| {
                let Block::Preformatted(text) = block else {
                    unreachable!("a plain text page")
                };
                div()
                    .font_family(theme::FONT_MONO)
                    .text_size(px(theme::TEXT_MD))
                    .line_height(px((theme::TEXT_MD * 1.6).round()))
                    .child(expand_tabs(text))
                    .into_any_element()
            })
            .collect();
    }
    let font_size = style.font_size();
    let spaces = spacing(blocks, font_size);
    blocks
        .iter()
        .zip(spaces)
        .map(|(block, (top, bottom))| {
            div()
                .mt(px(top))
                .mb(px(bottom))
                .child(block_view(block, style, font_size))
                .into_any_element()
        })
        .collect()
}

fn block_view(block: &Block, style: PageStyle, font_size: f32) -> AnyElement {
    match block {
        Block::Heading(level, inline) => {
            let size = heading_size(*level, font_size);
            div()
                .text_size(px(size))
                .line_height(px((size * 1.25).round()))
                .font_weight(FontWeight::BOLD)
                .child(styled_text(inline))
                .into_any_element()
        }
        Block::Paragraph(inline) => div().child(styled_text(inline)).into_any_element(),
        Block::Preformatted(text) => div()
            .px(px(0.3 * font_size))
            .py(px(0.12 * font_size))
            .rounded(px(theme::RADIUS_SM))
            .bg(rgb(theme::DOCUMENT_CODE))
            .font_family(theme::FONT_MONO)
            .child(expand_tabs(text))
            .into_any_element(),
        // The text page's `blockquote` rule: 16px in behind a 3px rule,
        // greyed. A book has no rule of its own, so the browser's 40px
        // margins on either side apply.
        Block::Quote(inner) => match style {
            PageStyle::Text => div()
                .pl(px(16.))
                .border_l(px(3.))
                .border_color(rgb(theme::DOCUMENT_QUOTE_RULE))
                .text_color(rgb(theme::DOCUMENT_QUOTE)),
            PageStyle::Book => div().mx(px(40.)),
        }
        .flex()
        .flex_col()
        .children(blocks_view(inner, style))
        .into_any_element(),
        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .children(items.iter().enumerate().map(|(index, item)| {
                let marker = match start {
                    Some(start) => format!("{}.", start + index as u64),
                    None => "•".to_owned(),
                };
                // `padding-left: 40px`, the marker outside the item's box.
                div()
                    .flex()
                    .child(
                        div()
                            .w(px(40.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .pr(px(6.))
                            .child(marker),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            // A tight list's paragraphs carry no margins.
                            .children(item.iter().map(|block| block_view(block, style, font_size))),
                    )
            }))
            .into_any_element(),
        Block::Table(rows) => div()
            .flex()
            .flex_col()
            .children(rows.iter().map(|row| {
                div().flex().children(
                    row.iter()
                        .map(|cell| div().flex_1().min_w_0().p(px(1.)).child(styled_text(cell))),
                )
            }))
            .into_any_element(),
        Block::Rule => div()
            .h(px(2.))
            .border_t_1()
            .border_color(rgb(theme::DOCUMENT_QUOTE_RULE))
            .into_any_element(),
        Block::Picture(picture) => img(picture.image.clone())
            .max_w_full()
            .w(px(picture.size.0))
            .h(px(picture.size.1))
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
    }
}

/// Inline styles as text highlights: bold, italic, struck, links in the
/// link colour and underlined, code in the monospace face on its tint.
fn styled_text(inline: &Inline) -> StyledText {
    let highlights: Vec<_> = inline
        .marks
        .iter()
        .map(|(range, marks)| (range.clone(), highlight(*marks)))
        .collect();
    let code: Vec<_> = inline
        .marks
        .iter()
        .filter(|(_, marks)| marks.code)
        .map(|(range, _)| (range.clone(), SharedString::from(theme::FONT_MONO)))
        .collect();
    StyledText::new(inline.text.clone())
        .with_highlights(highlights)
        .with_font_family_overrides(code)
}

fn highlight(marks: Marks) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    if marks.bold {
        style.font_weight = Some(FontWeight::BOLD);
    }
    if marks.italic {
        style.font_style = Some(FontStyle::Italic);
    }
    if marks.strike {
        style.strikethrough = Some(StrikethroughStyle {
            thickness: px(1.),
            color: None,
        });
    }
    if marks.link {
        let color: Hsla = rgb(theme::DOCUMENT_LINK).into();
        style.color = Some(color);
        style.underline = Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(color),
            wavy: false,
        });
    }
    if marks.code {
        style.background_color = Some(rgb(theme::DOCUMENT_CODE).into());
    }
    style
}

/// `tab-size: 4`: each tab runs to the next multiple of four columns.
pub fn expand_tabs(text: &str) -> String {
    if !text.contains('\t') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut column = 0;
    for character in text.chars() {
        match character {
            '\t' => {
                let spaces = 4 - column % 4;
                out.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            }
            '\n' => {
                out.push('\n');
                column = 0;
            }
            other => {
                out.push(other);
                column += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Block, Inline, expand_tabs, spacing};

    #[test]
    fn tabs_run_to_the_next_stop() {
        assert_eq!(expand_tabs("a\tb\n\tc"), "a   b\n    c");
    }

    #[test]
    fn margins_collapse_between_blocks() {
        let blocks = [
            Block::Heading(1, Inline::plain("Title")),
            Block::Paragraph(Inline::plain("one")),
            Block::Paragraph(Inline::plain("two")),
        ];
        let spaces = spacing(&blocks, 14.);
        // The first heading sits on the padding; the paragraph after it is
        // the larger of 0.55 × 28 and 14 below it; paragraphs are 14 apart
        // and the last keeps its bottom margin.
        assert_eq!(spaces[0], (0., 0.));
        assert!((spaces[1].0 - 15.4).abs() < 1e-4 && spaces[1].1 == 0.);
        assert_eq!(spaces[2], (14., 14.));
    }
}
