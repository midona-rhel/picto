//! The viewer of text documents (plain text, Markdown, JSON and RTF) and of
//! EPUB books. It draws the document on a white page inside the frame that
//! `shell.rs` builds: a viewport that scrolls and shows the page, and a
//! footer with the Previous page button, the text "Page n of m" and the Next
//! page button. Production does this in `TextDocumentViewer.tsx` and in the
//! EPUB part of `ArchiveDocumentViewer.tsx`.
//!
//! The file is read and parsed into blocks (`rich::Block`) on a background
//! thread. The viewer builds GPUI elements from the blocks, and GPUI lays
//! out their text. A text document is one page. A book shows one chapter at
//! a time as a page. The footer's buttons, and the Previous Page and Next
//! Page shortcuts, go to the chapter before it and to the chapter after it.
use super::kind::RendererKind;
use super::rich::{self, Block, Inline, Marks};
use super::shell;
use crate::theme;
use anyhow::{Result, anyhow};
use gpui::*;
use std::path::PathBuf;
use std::sync::Arc;

/// The number of bytes that are read of a text document: 4 MB, counted as
/// 4 × 1024 × 1024 bytes. Of a longer file only this first part is shown,
/// and a line under the text says "Only the first 4 MB of this file is
/// shown."
pub const TEXT_LIMIT: usize = 4 << 20;

/// Which of the two kinds of page the viewer draws. The sizes are those of
/// production's CSS.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PageStyle {
    /// The page of a text document (`.page` in production's
    /// `TextDocumentViewer.module.css`). It is as wide as the viewport has
    /// room for, and at most 816 px wide. It is at least as high as the
    /// viewport has room for, but this least height is never more than
    /// 1056 px. It has 72 px of padding above and below the text and 76 px
    /// of padding to the left and right of it. The text is 14 px in the UI
    /// font, with a line height of 1.65.
    Text,
    /// The page of a book (`.bookPage` in production's
    /// `ArchiveDocumentViewer.module.css`). It is as wide as the viewport
    /// has room for, and at most 760 px wide. It is at least as high as the
    /// viewport has room for. It has 56 px of padding above and below the
    /// text and 64 px of padding to the left and right of it. The text is
    /// 16 px in Georgia, with a line height of 1.65.
    Book,
}

impl PageStyle {
    /// Returns the largest width of the page in pixels.
    pub(super) fn max_width(self) -> f32 {
        match self {
            PageStyle::Text => 816.,
            PageStyle::Book => 760.,
        }
    }
    /// Returns the page's padding in pixels: first the padding above and
    /// below the text, then the padding to the left and right of it.
    pub(super) fn padding(self) -> (f32, f32) {
        match self {
            PageStyle::Text => (72., 76.),
            PageStyle::Book => (56., 64.),
        }
    }
    /// Returns the size of the page's text in pixels.
    pub(super) fn font_size(self) -> f32 {
        match self {
            PageStyle::Text => 14.,
            PageStyle::Book => 16.,
        }
    }
    /// Returns the name of the font family of the page's text.
    pub(super) fn font(self) -> &'static str {
        match self {
            PageStyle::Text => theme::FONT_UI,
            PageStyle::Book => theme::FONT_BOOK,
        }
    }
    /// Returns the colour of the page's text as `0xRRGGBB`.
    pub(super) fn color(self) -> u32 {
        match self {
            PageStyle::Text => theme::DOCUMENT_TEXT,
            PageStyle::Book => theme::BOOK_TEXT,
        }
    }
}

/// Reads the text document at `path` and parses it into blocks. `mime`, the
/// file's media type, decides the parser, as in production's
/// `TextDocumentViewer.tsx`. `text/markdown` is parsed as Markdown.
/// `application/json` is shown as plain text after the JSON has been
/// written out again with an indent of two spaces. A JSON file that cannot
/// be parsed, or that was cut off at `TEXT_LIMIT`, is shown as plain text
/// without that step. `application/rtf` and `text/rtf` are parsed as RTF.
/// Every other type is shown as plain text.
///
/// At most `TEXT_LIMIT` bytes of the file are read. Bytes that are not valid
/// UTF-8 are replaced by the character U+FFFD. The function returns the
/// blocks, and true when the file is longer than `TEXT_LIMIT`. It fails with
/// "Could not open this document." when the file cannot be opened.
pub fn load_text(path: &std::path::Path, mime: &str) -> Result<(Vec<Block>, bool)> {
    use std::io::Read;
    // i18n: an error the document view shows through t()
    let file = std::fs::File::open(path).map_err(|_| anyhow!("Could not open this document."))?;
    let mut bytes = Vec::new();
    file.take(TEXT_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > TEXT_LIMIT;
    bytes.truncate(TEXT_LIMIT);
    let text = String::from_utf8_lossy(&bytes);
    let blocks = match mime {
        "text/markdown" => rich::markdown(&text),
        "application/json" if !truncated => rich::json(&text),
        "application/rtf" | "text/rtf" => super::rtf::rtf(&text),
        _ => rich::plain_text(&text),
    };
    Ok((blocks, truncated))
}

/// The view that shows one text document or one book.
pub struct ReaderViewer {
    /// The file that is shown.
    path: PathBuf,
    /// The file's media type. `load_text` chooses the parser of a text
    /// document by it. A book does not use it.
    mime: String,
    /// Whether the file is drawn on a text page or on a book page. It is
    /// `Book` for an EPUB and `Text` for every other file.
    style: PageStyle,
    /// The chapters of a book in reading order, each as the path of its
    /// file inside the EPUB archive. It is empty for a text document, and
    /// for a book until the first chapter has loaded.
    chapters: Vec<String>,
    /// The index, counted from 0, of the chapter that is shown or is being
    /// loaded. It is always 0 for a text document.
    page: usize,
    /// The parsed content that is drawn on the page: the whole text
    /// document, or one chapter of a book. It is `None` until the first
    /// load has succeeded. While another chapter is being loaded, it still
    /// holds the chapter that was shown before.
    blocks: Option<Arc<Vec<Block>>>,
    /// True when the text document is longer than `TEXT_LIMIT`, so that
    /// only its first part is shown. It is always false for a book.
    truncated: bool,
    /// The message that the shell shows in place of the page when the file
    /// or the chapter could not be read. The next load that succeeds
    /// clears it.
    error: Option<SharedString>,
    /// The task that waits for the result of the background thread and
    /// stores it in the viewer. `load` puts a new task here each time.
    /// That drops the task of the load before it, which cancels that load.
    task: Option<Task<()>>,
    /// The scroll position of the shell's viewport. It is set back to the
    /// top each time a document or a chapter has loaded.
    scroll: ScrollHandle,
    /// The width and the height in pixels that the page has room for: the
    /// view's size less the footer's height and the padding around the
    /// page (`shell::page_room`). `build` sets it at its start. Both are 0
    /// before the first frame.
    available: (f32, f32),
}

impl ReaderViewer {
    /// Makes the viewer of the file at `path` and starts loading the file.
    /// A file of the kind `RendererKind::Epub` is shown as a book. A file
    /// of any other kind is shown as a text document, which is parsed by
    /// its media type `mime`.
    pub fn new(kind: RendererKind, path: PathBuf, mime: String, cx: &mut Context<Self>) -> Self {
        let style = if kind == RendererKind::Epub {
            PageStyle::Book
        } else {
            PageStyle::Text
        };
        // When the viewer is released, the pictures of the chapter that it
        // shows are taken out of GPUI's sprite atlas, which otherwise keeps
        // every picture that was drawn.
        cx.on_release(|this, cx| this.release_pictures(cx)).detach();
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

    /// Starts loading the content for `self.page` on a background thread,
    /// and stores the result in the viewer when it arrives. For a book, the
    /// thread reads the list of chapters if the viewer does not have it
    /// yet, and then reads and parses the chapter with the index
    /// `self.page`. For a text document, it reads and parses the file
    /// (`load_text`). The work runs inside `super::guarded`, so a parser
    /// that panics produces an error and does not end the app.
    ///
    /// When the content arrives, the pictures of the content that was shown
    /// before are taken out of the sprite atlas, the new content takes its
    /// place, the error message is cleared, and the viewport is scrolled
    /// to its top. When the load failed, the text of the error becomes the
    /// viewer's error message, which the shell shows in place of the page.
    fn load(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let mime = self.mime.clone();
        let style = self.style;
        let chapters = self.chapters.clone();
        let page = self.page;
        let work = cx.background_executor().spawn(async move {
            super::guarded(|| {
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
            })
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                // The result is ignored when the viewer has gone to another
                // chapter since this load started.
                if this.page != page {
                    return;
                }
                match result {
                    Ok((chapters, blocks, truncated)) => {
                        this.release_pictures(cx);
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

    /// Removes the content that is shown from the viewer, which leaves
    /// `blocks` as `None`, and takes every picture in that content out of
    /// GPUI's sprite atlas (`crate::atlas::release`). Only the chapters of
    /// a book have pictures.
    fn release_pictures(&mut self, cx: &mut App) {
        for picture in self
            .blocks
            .take()
            .iter()
            .flat_map(|blocks| rich::pictures(blocks))
        {
            crate::atlas::release(picture, cx);
        }
    }

    /// Goes to the chapter before the one that is shown and starts loading
    /// it. It does nothing at the first chapter of a book and for a text
    /// document.
    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        if self.page > 0 {
            self.page -= 1;
            self.load(cx);
        }
    }

    /// Goes to the chapter after the one that is shown and starts loading
    /// it. It does nothing at the last chapter of a book and for a text
    /// document.
    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        if self.page + 1 < self.chapters.len() {
            self.page += 1;
            self.load(cx);
        }
    }

    /// Returns true when the viewer has content to draw or an error
    /// message to show, which is the case once the first load has ended.
    /// The tests in `matrix.rs` that draw the viewers, which run on macOS
    /// only, wait for it.
    #[cfg(all(test, target_os = "macos"))]
    pub fn loaded(&self) -> bool {
        self.blocks.is_some() || self.error.is_some()
    }

    /// Returns the number of pages, which the footer shows as m in
    /// "Page n of m": the number of chapters for a book, and 1 for a text
    /// document. For a book it is 0 until the list of chapters has been
    /// read, and the footer then shows a dash in place of the number.
    fn count(&self) -> usize {
        if self.style == PageStyle::Book {
            self.chapters.len()
        } else {
            1
        }
    }
}

impl Render for ReaderViewer {
    /// The page's width and its smallest height depend on the size of the
    /// view, so `render` returns an element that fills the view's area and
    /// builds the page for that element's bounds in the frame that lays it
    /// out (`ui::sized_view`).
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::sized_view(cx.weak_entity(), Self::build)
            .absolute()
            .inset_0()
    }
}

impl ReaderViewer {
    /// Builds one frame of the viewer for `bounds`, the view's position and
    /// size in window coordinates in this frame.
    fn build(
        &mut self,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.available = shell::page_room(bounds.size);
        let style = self.style;
        let page = self.blocks.as_ref().map(|blocks| {
            let (vertical, horizontal) = style.padding();
            // The page is as wide as the room it has, and at most as wide
            // as `style.max_width()`. Production's CSS has
            // `width: min(816px, 100%)` for a text page and
            // `width: min(100%, 760px)` for a book page. The page is 0 wide
            // when it has no room.
            let width = super::geometry::bound_page_width(
                self.available.0,
                (style.max_width(), 1.),
                style.max_width(),
            )
            .map_or(0., |size| size.0);
            // The page is at least as high as the room it has. For a text
            // page this least height is at most 1056 px. Production's CSS
            // has `min-height: min(1056px, 100cqh)` for a text page and
            // `min-height: 100%` for a book page.
            let min_height = match style {
                PageStyle::Text => self.available.1.min(1056.),
                PageStyle::Book => self.available.1,
            };
            let mut children = blocks_view(blocks, style);
            // Under a text document of which only the first `TEXT_LIMIT`
            // bytes were read, a line in grey italic text says so. The
            // space above the line is as high as the page's font size.
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
            // The page is a white rectangle with the blocks one under the
            // other inside its padding. A text page has the shadow
            // `theme::page_shadow` and a book page has the shadow
            // `theme::sheet_shadow`. The line height is 1.65 times the font
            // size, rounded to a whole pixel: 23 px on a text page and
            // 26 px on a book page.
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
        // The shell shows the page, or the error message in place of the
        // page, and "Page n of m" in its footer. Its two buttons go to the
        // chapter before and to the chapter after the one that is shown.
        // The shell disables the Previous page button on the first page
        // and the Next page button on the last page, so both are disabled
        // for a text document.
        let entity = cx.entity().downgrade();
        let (previous, next) = (entity.clone(), entity);
        let shell = shell::shell(&self.scroll, self.page + 1, self.count())
            .error(self.error.clone())
            .content(page)
            .on_previous(move |_, cx| {
                let _ = previous.update(cx, |this, cx| this.previous_page(cx));
            })
            .on_next(move |_, cx| {
                let _ = next.update(cx, |this, cx| this.next_page(cx));
            });
        // The shell places itself over the whole of its parent, so it is
        // given a parent that has the view's size.
        div().relative().size_full().child(shell).into_any_element()
    }
}

/// Returns the margin in pixels above `block` and the margin below it. A
/// heading on a text page has a margin of 1.4 times its own font size above
/// it and 0.55 times its own font size below it (`margin: 1.4em 0 0.55em`
/// in production's `TextDocumentViewer.module.css`). Every other block on a
/// text page, and every block on a book page, has no margin. That is
/// because production's stylesheet starts with a rule that sets the margin
/// and the padding of every element to 0 (`src/app/globals.css`), and only
/// the rule for the headings of a text page sets a margin after that.
/// `font_size` is the size of the page's text in pixels.
pub fn margins(block: &Block, font_size: f32, style: PageStyle) -> (f32, f32) {
    match (block, style) {
        (Block::Heading(level, _), PageStyle::Text) => {
            let size = heading_size(*level, font_size);
            (1.4 * size, 0.55 * size)
        }
        _ => (0., 0.),
    }
}

/// Returns the font size in pixels of a heading of `level` on a page whose
/// text is `font_size` pixels. The factors are those a browser gives the
/// headings `h1` to `h6` by default: 2 for level 1, 1.5 for level 2, 1.17
/// for level 3, 1 for level 4, 0.83 for level 5 and 0.67 for level 6. Any
/// other level also gets 0.67.
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

/// Returns, for each block in `blocks`, the space in pixels to leave above
/// it and the space to leave below it. The spaces follow the CSS rule that
/// the margins of two blocks which touch are not added: the space between
/// two blocks is the larger of the upper block's bottom margin and the
/// lower block's top margin (`margins`). That space is returned as the
/// space above the lower block. The space below is 0 for every block
/// except the last one, which gets its own bottom margin.
///
/// The space above the first block is its own top margin. It is 0 when the
/// first block is a heading of level 1 or 2, as in production's rule
/// `.page h1:first-child, .page h2:first-child { margin-top: 0 }`.
pub fn spacing(blocks: &[Block], font_size: f32, style: PageStyle) -> Vec<(f32, f32)> {
    blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let (top, bottom) = margins(block, font_size, style);
            let above = match index {
                0 if matches!(block, Block::Heading(1 | 2, _)) => 0.,
                0 => top,
                _ => top.max(margins(&blocks[index - 1], font_size, style).1),
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

/// Returns true when `blocks` is not empty and every block is preformatted
/// text. A plain text file and a JSON file arrive like that, cut into
/// blocks of 64 lines (`rich::plain_text`). Production draws such a file as
/// a single `<pre>` element with the class `.plainText`, and a text page
/// draws these blocks the same way: without margins, in 13 px monospace
/// text with a line height of 1.6, and with long lines wrapped.
pub(super) fn plain_text_page(blocks: &[Block]) -> bool {
    !blocks.is_empty()
        && blocks
            .iter()
            .all(|block| matches!(block, Block::Preformatted(_)))
}

/// Returns one element for each block in `blocks`, in order, for a page of
/// `style`. On a text page whose blocks are all preformatted text
/// (`plain_text_page`), each block is drawn in 13 px monospace text with a
/// line height of 1.6, rounded to 21 px, with its tabs expanded
/// (`expand_tabs`) and no space between the blocks. Otherwise each block is
/// drawn by `block_view` with the space above and below it that `spacing`
/// returns.
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
    let spaces = spacing(blocks, font_size, style);
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

/// Returns the element that draws `block` on a page of `style` whose text
/// is `font_size` pixels. The element has no space above or below it;
/// `blocks_view` adds that.
fn block_view(block: &Block, style: PageStyle, font_size: f32) -> AnyElement {
    match block {
        Block::Heading(level, inline) => {
            let size = heading_size(*level, font_size);
            // A heading is bold. On a text page its line height is 1.25
            // times its font size (`line-height: 1.25` in the heading rule
            // of production's `.page`). On a book page it is 1.65 times
            // its font size, as for the rest of the page.
            let line = match style {
                PageStyle::Text => 1.25,
                PageStyle::Book => 1.65,
            };
            div()
                .text_size(px(size))
                .line_height(px((size * line).round()))
                .font_weight(FontWeight::BOLD)
                .child(styled_text(inline, style))
                .into_any_element()
        }
        Block::Paragraph(inline) => div().child(styled_text(inline, style)).into_any_element(),
        // Preformatted text, such as a code block in Markdown (`<pre><code>`
        // in HTML), is drawn in the monospace font with its tabs expanded
        // (`expand_tabs`). On a text page it also gets what production's
        // rule `.page code` gives it: the background `#eef0f2`, padding of
        // 0.12 times the font size above and below and 0.3 times the font
        // size left and right, and corners rounded by 4 px. That box is as
        // wide as the longest line of the text, not as wide as the page,
        // which is why it is the child of a flex row. A book page has no
        // rule for `code`, so there the text has no background and no
        // padding.
        Block::Preformatted(text) => {
            let code = div().font_family(theme::FONT_MONO).child(expand_tabs(text));
            let code = match style {
                PageStyle::Text => code
                    .px(px(0.3 * font_size))
                    .py(px(0.12 * font_size))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(rgb(theme::DOCUMENT_CODE)),
                PageStyle::Book => code,
            };
            div().flex().child(code).into_any_element()
        }
        // A quote on a text page has a vertical line 3 px wide in the
        // colour `#c7c9cd` along its left edge, 16 px of padding between
        // the line and the text, and text in the colour `#5b5e65`
        // (production's rule `.page blockquote`). A quote on a book page
        // is drawn like the text around it, without a line and without an
        // indent: `.bookPage` has no rule for `blockquote`, and the reset
        // at the start of production's stylesheet removes the margins that
        // a browser gives a quote by default.
        Block::Quote(inner) => match style {
            PageStyle::Text => div()
                .pl(px(16.))
                .border_l(px(3.))
                .border_color(rgb(theme::DOCUMENT_QUOTE_RULE))
                .text_color(rgb(theme::DOCUMENT_QUOTE)),
            PageStyle::Book => div(),
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
                // The marker of an item is its number followed by a full
                // stop in a numbered list, which counts up from `start`,
                // and a bullet in any other list. The reset at the start
                // of production's stylesheet removes the padding of a
                // list, so an item is as wide as the page's text and its
                // marker is drawn outside it: the marker ends 6 px to the
                // left of the item's left edge, on the item's first line.
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right(relative(1.))
                            .pr(px(6.))
                            .whitespace_nowrap()
                            .child(marker),
                    )
                    .children(item.iter().map(|block| block_view(block, style, font_size)))
            }))
            .into_any_element(),
        // A table is drawn as a browser draws one whose cells have no
        // padding, which the reset at the start of production's stylesheet
        // removes. Each column is as wide as its widest cell. There are
        // 2 px between two columns, between two rows and around the whole
        // table, which is a browser's default `border-spacing`. The table
        // is built as a row of columns, each column holding its cell of
        // every row, so that the cells of a column have the same left
        // edge. Each column is laid out on its own. The cells of one row
        // are at the same height in every column because the text of a
        // cell does not wrap, which makes a cell one line high. A row that
        // has fewer cells than the table has columns gets an element
        // without text in each column that it lacks.
        Block::Table(rows) => {
            let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
            div()
                .flex()
                .p(px(2.))
                .gap(px(2.))
                .children((0..columns).map(|column| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .children(rows.iter().map(|row| {
                            div()
                                .whitespace_nowrap()
                                .children(row.get(column).map(|cell| styled_text(cell, style)))
                        }))
                }))
                .into_any_element()
        }
        // A horizontal rule is a box 2 px high across the page's text, with
        // a line 1 px thick in the colour `#c7c9cd` along its top edge.
        Block::Rule => div()
            .h(px(2.))
            .border_t_1()
            .border_color(rgb(theme::DOCUMENT_QUOTE_RULE))
            .into_any_element(),
        // A picture is drawn at its own width and height in pixels. A
        // picture that is wider than the element that contains it, which
        // is as wide as the page's text unless the picture is in a quote
        // or a list, is scaled down to that width and keeps its
        // proportions.
        Block::Picture(picture) => img(picture.image.clone())
            .max_w_full()
            .w(px(picture.size.0))
            .h(px(picture.size.1))
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
    }
}

/// Returns the text of `inline` with the styles of its marked ranges, for
/// a page of `style`. `highlight` gives the style of each range. A range
/// that is marked as code is also drawn in the monospace font.
fn styled_text(inline: &Inline, style: PageStyle) -> StyledText {
    let highlights: Vec<_> = inline
        .marks
        .iter()
        .map(|(range, marks)| (range.clone(), highlight(*marks, style)))
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

/// Returns the style of a range of text that has `marks`, on a page of
/// `page`. Bold text has the bold weight. Italic text is italic. Struck
/// text has a line 1 px thick through it. A link is in the colour `#2773c9`
/// and has an underline 1 px thick in that colour. Code has the background
/// `#eef0f2` on a text page and no background on a book page, because
/// production's CSS has a rule for `code` only on the text page.
fn highlight(marks: Marks, page: PageStyle) -> HighlightStyle {
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
    if marks.code && page == PageStyle::Text {
        style.background_color = Some(rgb(theme::DOCUMENT_CODE).into());
    }
    style
}

/// Returns `text` with each tab character replaced by one to four spaces,
/// so that the character after the tab is at the next column that is a
/// multiple of 4 (`tab-size: 4` in production's `.plainText`). Columns are
/// counted in characters from the start of the line. Text without a tab
/// is returned as it is.
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
    use super::{Block, Inline, PageStyle, expand_tabs, spacing};

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
        let spaces = spacing(&blocks, 14., PageStyle::Text);
        // The heading is the page's first block and has level 1, so there
        // is no space above it. The space above the first paragraph is the
        // heading's bottom margin: a heading of level 1 on a page with
        // 14 px text is 28 px, and 0.55 × 28 px is 15.4 px. There is no
        // space above the second paragraph, because a paragraph has no
        // margins.
        assert_eq!(spaces[0], (0., 0.));
        assert!((spaces[1].0 - 15.4).abs() < 1e-4 && spaces[1].1 == 0.);
        assert_eq!(spaces[2], (0., 0.));
    }

    #[test]
    fn a_book_has_no_margins() {
        let blocks = [
            Block::Paragraph(Inline::plain("one")),
            Block::Heading(2, Inline::plain("Two")),
            Block::Paragraph(Inline::plain("three")),
        ];
        let spaces = spacing(&blocks, 16., PageStyle::Book);
        assert!(spaces.iter().all(|space| *space == (0., 0.)));
    }

    /// Checks that when the viewer goes to the next chapter of a book, the
    /// picture of the chapter that was shown before is taken out of the
    /// window's sprite atlas. The test opens the fixture `book.epub`,
    /// whose first chapter has one picture, draws it, goes to the second
    /// chapter and draws again.
    #[test]
    fn a_chapter_left_takes_its_pictures_out_of_the_atlas() {
        use super::{ReaderViewer, RendererKind, rich};
        use gpui::{AnyWindowHandle, AppContext as _, TestAppContext};
        let book = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/fixtures/documents/book.epub");
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_global(crate::theme::DARK));
        let window = cx.add_window(|_, cx| {
            ReaderViewer::new(RendererKind::Epub, book, "application/epub+zip".into(), cx)
        });
        let handle: AnyWindowHandle = window.into();
        let draw = |cx: &mut TestAppContext| {
            for _ in 0..2 {
                cx.run_until_parked();
                cx.update_window(handle, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                })
                .unwrap();
            }
        };
        draw(&mut cx);
        let pictures = window
            .read_with(&cx, |reader, _| {
                rich::pictures(reader.blocks.as_ref().unwrap())
            })
            .unwrap();
        assert_eq!(pictures.len(), 1);
        let in_atlas = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, _| {
                window.has_image_atlas_entry(&pictures[0])
            })
            .unwrap()
        };
        assert!(in_atlas(&mut cx));
        window
            .update(&mut cx, |reader, _, cx| reader.next_page(cx))
            .unwrap();
        draw(&mut cx);
        assert_eq!(window.read_with(&cx, |reader, _| reader.page).unwrap(), 1);
        assert!(!in_atlas(&mut cx));
    }
}
