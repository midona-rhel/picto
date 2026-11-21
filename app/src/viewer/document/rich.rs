//! The content of a text document, or of a chapter of a book, as blocks of
//! styled text. The blocks are built on a background thread from plain text,
//! JSON, Markdown and the XHTML of an EPUB's chapters by the functions here,
//! and from RTF by `super::rtf`. [`super::reader`] lays them out with GPUI's
//! text.
use gpui::RenderImage;
use std::ops::Range;
use std::sync::Arc;

/// The styles of a range of text inside a paragraph.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: bool,
    pub strike: bool,
}

/// The text of a paragraph, a heading or a table cell, and the ranges of
/// that text which are styled.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Inline {
    pub text: String,
    /// The byte ranges of `text` that are styled, each with its styles.
    /// They are in order and do not overlap. The text outside them is
    /// plain.
    pub marks: Vec<(Range<usize>, Marks)>,
}

impl Inline {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            marks: Vec::new(),
        }
    }

    fn push(&mut self, text: &str, marks: Marks) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        if marks != Marks::default() {
            match self.marks.last_mut() {
                Some((range, last)) if *last == marks && range.end == start => {
                    range.end = self.text.len()
                }
                _ => self.marks.push((start..self.text.len(), marks)),
            }
        }
    }

    fn trim_end(&mut self) {
        let trimmed = self.text.trim_end().len();
        self.text.truncate(trimmed);
        self.marks.retain_mut(|(range, _)| {
            range.end = range.end.min(trimmed);
            range.start < range.end
        });
    }

    fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// A picture in a chapter of a book, decoded into the form that GPUI draws.
/// `size` is its width and height in pixels.
#[derive(Clone)]
pub struct Picture {
    pub image: Arc<RenderImage>,
    pub size: (f32, f32),
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({}×{})", self.size.0, self.size.1)
    }
}

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.image, &other.image)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Block {
    Heading(u8, Inline),
    Paragraph(Inline),
    /// Text whose spaces and line breaks are kept as they are, drawn in the
    /// monospace font. A `<pre>` element of HTML and a code block of
    /// Markdown become this.
    Preformatted(String),
    Quote(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Table(Vec<Vec<Inline>>),
    Rule,
    Picture(Picture),
}

/// The number of lines in one preformatted block of a plain text file. A
/// long file is cut into blocks of this many lines, so that its text is
/// laid out as several blocks and not as one.
const PREFORMATTED_LINES: usize = 64;

/// Returns a plain text file as preformatted blocks of `PREFORMATTED_LINES`
/// lines each. A carriage return followed by a line feed is first replaced
/// by a line feed.
pub fn plain_text(text: &str) -> Vec<Block> {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    lines
        .chunks(PREFORMATTED_LINES)
        .map(|chunk| Block::Preformatted(chunk.join("\n")))
        .collect()
}

/// Returns a JSON file as preformatted blocks. The JSON is written out
/// again with an indent of two spaces. Text that is not valid JSON is
/// returned as it is.
pub fn json(text: &str) -> Vec<Block> {
    match serde_json::from_str::<serde_json::Value>(text)
        .and_then(|value| serde_json::to_string_pretty(&value))
    {
        Ok(pretty) => plain_text(&pretty),
        Err(_) => plain_text(text),
    }
}

// The following builds blocks from the calls that a parser makes.

enum Container {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Item(Vec<Block>),
    Table {
        rows: Vec<Vec<Inline>>,
    },
    Row(Vec<Inline>),
}

/// Builds blocks from a sequence of calls, each of which starts an element,
/// ends an element or adds text. The Markdown parser (`markdown`) and the
/// walk over a chapter's XHTML (`walk`) both make such calls.
struct Builder {
    stack: Vec<Container>,
    /// The paragraph or heading that text is being added to, with its
    /// level. Level 0 is a paragraph. While `cell` holds a table cell, text
    /// is added to that cell instead.
    open: Option<(u8, Inline)>,
    cell: Option<Inline>,
    marks: Marks,
    /// True when each run of whitespace in added text is replaced by one
    /// space, as HTML does. It is true for XHTML and false for Markdown.
    collapse: bool,
    /// The number of quotes, lists and list items that were started beyond
    /// [`MAX_NESTING`] levels and have not ended yet. They add no level,
    /// and their content is added to the deepest level that exists.
    flattened: usize,
    /// The depth of the element that the walk over a chapter's XHTML is at.
    depth: usize,
}

/// The largest number of levels to which quotes and lists are nested. A
/// file can nest them to any depth. The functions that lay blocks out and
/// free them call themselves once for each level, so blocks that are nested
/// too deeply would overflow the stack.
const MAX_NESTING: usize = 32;
/// The largest depth of elements that the walk over a chapter's XHTML goes
/// to. Of an element at that depth, only the text is taken, without the
/// structure of the elements inside it.
const MAX_DEPTH: usize = 128;

impl Builder {
    fn new(collapse: bool) -> Self {
        Self {
            stack: vec![Container::Root(Vec::new())],
            open: None,
            cell: None,
            marks: Marks::default(),
            collapse,
            flattened: 0,
            depth: 0,
        }
    }

    /// Returns true when a quote, list or list item that starts now must
    /// not add a level, because [`MAX_NESTING`] has been reached. It then
    /// counts the start in `flattened`, so that `flattened_end` can tell
    /// which end belongs to it.
    fn flatten(&mut self) -> bool {
        let flatten = self.flattened > 0 || self.stack.len() > MAX_NESTING;
        if flatten {
            self.close_inline();
            self.flattened += 1;
        }
        flatten
    }

    /// Returns true when the end of a quote, list or list item belongs to
    /// a start that [`Self::flatten`] counted, and takes that start off the
    /// count.
    fn flattened_end(&mut self) -> bool {
        let flattened = self.flattened > 0;
        if flattened {
            self.close_inline();
            self.flattened -= 1;
        }
        flattened
    }

    fn blocks(&mut self) -> &mut Vec<Block> {
        if !matches!(
            self.stack.last(),
            Some(Container::Root(_) | Container::Quote(_) | Container::Item(_))
        ) {
            // A block that is added directly inside a list goes into a new
            // list item. A block that is added directly inside a table or
            // a row goes into a new quote.
            match self.stack.last() {
                Some(Container::List { .. }) => self.stack.push(Container::Item(Vec::new())),
                _ => self.stack.push(Container::Quote(Vec::new())),
            }
        }
        match self.stack.last_mut() {
            Some(Container::Root(blocks) | Container::Quote(blocks) | Container::Item(blocks)) => {
                blocks
            }
            _ => unreachable!("a block container is on top"),
        }
    }

    fn push_block(&mut self, block: Block) {
        self.close_inline();
        self.blocks().push(block);
    }

    /// Ends the paragraph or heading that text is being added to, and adds
    /// it to the blocks unless it holds only whitespace.
    fn close_inline(&mut self) {
        if let Some((level, mut inline)) = self.open.take() {
            inline.trim_end();
            if !inline.is_blank() {
                let block = if level == 0 {
                    Block::Paragraph(inline)
                } else {
                    Block::Heading(level, inline)
                };
                self.blocks().push(block);
            }
        }
    }

    fn open_inline(&mut self, level: u8) {
        self.close_inline();
        self.open = Some((level, Inline::default()));
    }

    fn text(&mut self, text: &str) {
        let collapse = self.collapse;
        let marks = self.marks;
        let target = if let Some(cell) = &mut self.cell {
            cell
        } else {
            if self.open.is_none() {
                if collapse && text.trim().is_empty() {
                    return;
                }
                self.open = Some((0, Inline::default()));
            }
            &mut self.open.as_mut().expect("an open paragraph").1
        };
        if collapse {
            let mut collapsed = String::with_capacity(text.len());
            let mut space = target.text.is_empty() || target.text.ends_with([' ', '\n']);
            for character in text.chars() {
                if character.is_whitespace() && character != '\u{a0}' {
                    if !space {
                        collapsed.push(' ');
                        space = true;
                    }
                } else {
                    collapsed.push(character);
                    space = false;
                }
            }
            target.push(&collapsed, marks);
        } else {
            target.push(text, marks);
        }
    }

    fn line_break(&mut self) {
        let marks = self.marks;
        if let Some(cell) = &mut self.cell {
            cell.push("\n", marks);
        } else if let Some((_, inline)) = &mut self.open {
            inline.trim_end();
            inline.push("\n", marks);
        }
    }

    fn start_quote(&mut self) {
        if self.flatten() {
            return;
        }
        self.close_inline();
        self.stack.push(Container::Quote(Vec::new()));
    }

    fn end_quote(&mut self) {
        if self.flattened_end() {
            return;
        }
        self.close_inline();
        self.close_items();
        if let Some(Container::Quote(blocks)) = self.stack.pop() {
            self.blocks().push(Block::Quote(blocks));
        }
    }

    fn start_list(&mut self, start: Option<u64>) {
        if self.flatten() {
            return;
        }
        self.close_inline();
        self.stack.push(Container::List {
            start,
            items: Vec::new(),
        });
    }

    fn end_list(&mut self) {
        if self.flattened_end() {
            return;
        }
        self.close_inline();
        if matches!(self.stack.last(), Some(Container::Item(_))) {
            self.end_item();
        }
        if let Some(Container::List { start, items }) = self.stack.pop() {
            self.blocks().push(Block::List { start, items });
        }
    }

    fn start_item(&mut self) {
        if self.flatten() {
            return;
        }
        self.close_inline();
        if matches!(self.stack.last(), Some(Container::Item(_))) {
            self.end_item();
        }
        if !matches!(self.stack.last(), Some(Container::List { .. })) {
            self.stack.push(Container::List {
                start: None,
                items: Vec::new(),
            });
        }
        self.stack.push(Container::Item(Vec::new()));
    }

    fn end_item(&mut self) {
        if self.flattened_end() {
            return;
        }
        self.close_inline();
        if let Some(Container::Item(blocks)) = self.stack.pop() {
            if let Some(Container::List { items, .. }) = self.stack.last_mut() {
                items.push(blocks);
            }
        }
    }

    /// Ends the lists and list items that are still open inside a quote
    /// that is ending.
    fn close_items(&mut self) {
        while matches!(
            self.stack.last(),
            Some(Container::Item(_) | Container::List { .. })
        ) {
            if matches!(self.stack.last(), Some(Container::Item(_))) {
                self.end_item();
            } else {
                self.end_list();
            }
        }
    }

    fn start_table(&mut self) {
        self.close_inline();
        self.stack.push(Container::Table { rows: Vec::new() });
    }

    fn end_table(&mut self) {
        self.end_row();
        if let Some(Container::Table { rows }) = self.stack.pop() {
            if !rows.is_empty() {
                self.blocks().push(Block::Table(rows));
            }
        }
    }

    fn start_row(&mut self) {
        self.end_row();
        if matches!(self.stack.last(), Some(Container::Table { .. })) {
            self.stack.push(Container::Row(Vec::new()));
        }
    }

    fn end_row(&mut self) {
        self.end_cell();
        if matches!(self.stack.last(), Some(Container::Row(_)))
            && let Some(Container::Row(cells)) = self.stack.pop()
            && let Some(Container::Table { rows }) = self.stack.last_mut()
        {
            rows.push(cells);
        }
    }

    fn start_cell(&mut self) {
        self.end_cell();
        if !matches!(self.stack.last(), Some(Container::Row(_))) {
            self.start_row();
        }
        self.cell = Some(Inline::default());
    }

    fn end_cell(&mut self) {
        if let Some(mut cell) = self.cell.take() {
            cell.trim_end();
            if let Some(Container::Row(cells)) = self.stack.last_mut() {
                cells.push(cell);
            }
        }
    }

    fn finish(mut self) -> Vec<Block> {
        self.flattened = 0;
        self.close_inline();
        self.end_cell();
        while self.stack.len() > 1 {
            match self.stack.last() {
                Some(Container::Quote(_)) => self.end_quote(),
                Some(Container::List { .. }) => self.end_list(),
                Some(Container::Item(_)) => self.end_item(),
                Some(Container::Table { .. }) => self.end_table(),
                Some(Container::Row(_)) => self.end_row(),
                _ => break,
            }
        }
        match self.stack.pop() {
            Some(Container::Root(blocks)) => blocks,
            _ => Vec::new(),
        }
    }
}

// The following parses Markdown.

/// Parses Markdown into blocks. It understands tables and strikethrough,
/// which GitHub's flavour of Markdown adds.
pub fn markdown(source: &str) -> Vec<Block> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let mut builder = Builder::new(false);
    let mut code: Option<String> = None;
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => builder.open_inline(0),
                Tag::Heading { level, .. } => builder.open_inline(level as u8),
                Tag::BlockQuote(_) => builder.start_quote(),
                Tag::CodeBlock(_) => {
                    builder.close_inline();
                    code = Some(String::new());
                }
                Tag::List(start) => builder.start_list(start),
                Tag::Item => builder.start_item(),
                Tag::Table(_) => builder.start_table(),
                Tag::TableHead | Tag::TableRow => builder.start_row(),
                Tag::TableCell => builder.start_cell(),
                Tag::Emphasis => builder.marks.italic = true,
                Tag::Strong => builder.marks.bold = true,
                Tag::Strikethrough => builder.marks.strike = true,
                Tag::Link { .. } => builder.marks.link = true,
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) => builder.close_inline(),
                TagEnd::BlockQuote(_) => builder.end_quote(),
                TagEnd::CodeBlock => {
                    if let Some(mut text) = code.take() {
                        if text.ends_with('\n') {
                            text.pop();
                        }
                        builder.push_block(Block::Preformatted(text));
                    }
                }
                TagEnd::List(_) => builder.end_list(),
                TagEnd::Item => builder.end_item(),
                TagEnd::Table => builder.end_table(),
                TagEnd::TableHead | TagEnd::TableRow => builder.end_row(),
                TagEnd::TableCell => builder.end_cell(),
                TagEnd::Emphasis => builder.marks.italic = false,
                TagEnd::Strong => builder.marks.bold = false,
                TagEnd::Strikethrough => builder.marks.strike = false,
                TagEnd::Link => builder.marks.link = false,
                _ => {}
            },
            // The parser gives the alternative text of an image as text, so
            // that text is shown in place of the image. The viewer does not
            // load the pictures that a Markdown file refers to.
            Event::Text(text) => match &mut code {
                Some(code) => code.push_str(&text),
                None => builder.text(&text),
            },
            Event::Code(text) => {
                builder.marks.code = true;
                builder.text(&text);
                builder.marks.code = false;
            }
            Event::SoftBreak => builder.text(" "),
            Event::HardBreak => builder.line_break(),
            Event::Rule => builder.push_block(Block::Rule),
            // Of raw HTML in the Markdown, the tags are removed and the
            // text between them is shown.
            Event::Html(html) | Event::InlineHtml(html) => {
                let text = strip_tags(&html);
                if !text.trim().is_empty() {
                    builder.text(&text);
                }
            }
            _ => {}
        }
    }
    builder.finish()
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for character in html.chars() {
        match character {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(character),
            _ => {}
        }
    }
    out
}

// The following parses the XHTML of an EPUB's chapters.

/// Named entities that HTML defines and XML does not, each with the numeric
/// reference that replaces it before a chapter is parsed as XML. They are
/// the ones that chapters commonly use.
const ENTITIES: [(&str, &str); 12] = [
    ("&nbsp;", "&#160;"),
    ("&mdash;", "&#8212;"),
    ("&ndash;", "&#8211;"),
    ("&hellip;", "&#8230;"),
    ("&lsquo;", "&#8216;"),
    ("&rsquo;", "&#8217;"),
    ("&ldquo;", "&#8220;"),
    ("&rdquo;", "&#8221;"),
    ("&copy;", "&#169;"),
    ("&reg;", "&#174;"),
    ("&shy;", "&#173;"),
    ("&eacute;", "&#233;"),
];

/// Parses the XHTML of a chapter and returns the content of its body as
/// blocks. `picture` is called with the source address of each image and
/// returns the decoded picture, or `None` when the image is to be left out.
/// Scripts, styles, forms and embedded content are left out. A chapter that
/// is not well-formed XML is returned as paragraphs of its text without the
/// tags.
pub fn xhtml(markup: &str, picture: &mut dyn FnMut(&str) -> Option<Picture>) -> Vec<Block> {
    let mut source = markup.to_owned();
    for (entity, number) in ENTITIES {
        if source.contains(entity) {
            source = source.replace(entity, number);
        }
    }
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let Ok(document) = roxmltree::Document::parse_with_options(&source, options) else {
        // The chapter is not well-formed XML. Its text without the tags is
        // shown as paragraphs, so that the page is not empty.
        return strip_tags(markup)
            .split("\n\n")
            .filter(|paragraph| !paragraph.trim().is_empty())
            .map(|paragraph| {
                Block::Paragraph(Inline::plain(
                    paragraph.split_whitespace().collect::<Vec<_>>().join(" "),
                ))
            })
            .collect();
    };
    let root = document.root_element();
    let body = root
        .descendants()
        .find(|node| node.tag_name().name().eq_ignore_ascii_case("body"))
        .unwrap_or(root);
    let mut builder = Builder::new(true);
    walk(body, &mut builder, picture, false);
    builder.finish()
}

fn walk(
    node: roxmltree::Node,
    builder: &mut Builder,
    picture: &mut dyn FnMut(&str) -> Option<Picture>,
    preformatted: bool,
) {
    if builder.depth >= MAX_DEPTH {
        let text: String = node
            .descendants()
            .filter_map(|node| node.text().filter(|_| node.is_text()))
            .collect();
        builder.text(&text);
        return;
    }
    builder.depth += 1;
    walk_children(node, builder, picture, preformatted);
    builder.depth -= 1;
}

fn walk_children(
    node: roxmltree::Node,
    builder: &mut Builder,
    picture: &mut dyn FnMut(&str) -> Option<Picture>,
    preformatted: bool,
) {
    for child in node.children() {
        if child.is_text() {
            let text = child.text().unwrap_or("");
            if preformatted {
                builder.collapse = false;
                builder.text(text);
                builder.collapse = true;
            } else {
                builder.text(text);
            }
            continue;
        }
        if !child.is_element() {
            continue;
        }
        let name = child.tag_name().name().to_ascii_lowercase();
        match name.as_str() {
            "script" | "style" | "head" | "title" | "iframe" | "object" | "embed" | "form"
            | "link" | "meta" | "base" => {}
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                builder.open_inline(name.as_bytes()[1] - b'0');
                walk(child, builder, picture, preformatted);
                builder.close_inline();
            }
            "p" | "div" | "section" | "article" | "header" | "footer" | "nav" | "aside"
            | "main" | "figure" | "figcaption" | "dl" | "dt" | "dd" | "address" | "center"
            | "body" => {
                builder.close_inline();
                walk(child, builder, picture, preformatted);
                builder.close_inline();
            }
            "blockquote" => {
                builder.start_quote();
                walk(child, builder, picture, preformatted);
                builder.end_quote();
            }
            "ul" | "ol" => {
                let start = (name == "ol").then(|| {
                    child
                        .attribute("start")
                        .and_then(|start| start.parse().ok())
                        .unwrap_or(1)
                });
                builder.start_list(start);
                walk(child, builder, picture, preformatted);
                builder.end_list();
            }
            "li" => {
                builder.start_item();
                walk(child, builder, picture, preformatted);
                builder.end_item();
            }
            "pre" => {
                builder.close_inline();
                let text: String = child
                    .descendants()
                    .filter(|node| node.is_text())
                    .filter_map(|node| node.text())
                    .collect();
                builder.push_block(Block::Preformatted(text.trim_end_matches('\n').to_owned()));
            }
            "hr" => builder.push_block(Block::Rule),
            "br" => builder.line_break(),
            "table" => {
                builder.start_table();
                walk(child, builder, picture, preformatted);
                builder.end_table();
            }
            "tr" => {
                builder.start_row();
                walk(child, builder, picture, preformatted);
                builder.end_row();
            }
            "td" | "th" => {
                builder.start_cell();
                let bold = builder.marks.bold;
                builder.marks.bold |= name == "th";
                walk(child, builder, picture, preformatted);
                builder.marks.bold = bold;
                builder.end_cell();
            }
            "img" | "image" => {
                let source = child
                    .attribute("src")
                    .or_else(|| child.attribute(("http://www.w3.org/1999/xlink", "href")))
                    .or_else(|| child.attribute("href"));
                if let Some(found) = source.and_then(&mut *picture) {
                    builder.push_block(Block::Picture(found));
                }
            }
            "svg" => {
                // A cover is often an `<svg>` element that holds one
                // `<image>` element, so the walk goes into it to find the
                // image.
                walk(child, builder, picture, preformatted);
            }
            "b" | "strong" => styled(child, builder, picture, preformatted, |marks| {
                marks.bold = true
            }),
            "i" | "em" | "cite" | "var" => styled(child, builder, picture, preformatted, |marks| {
                marks.italic = true
            }),
            "code" | "tt" | "kbd" | "samp" => {
                styled(child, builder, picture, preformatted, |marks| {
                    marks.code = true
                })
            }
            "a" => styled(child, builder, picture, preformatted, |marks| {
                marks.link = true
            }),
            "s" | "strike" | "del" => styled(child, builder, picture, preformatted, |marks| {
                marks.strike = true
            }),
            _ => walk(child, builder, picture, preformatted),
        }
    }
}

fn styled(
    node: roxmltree::Node,
    builder: &mut Builder,
    picture: &mut dyn FnMut(&str) -> Option<Picture>,
    preformatted: bool,
    mark: impl Fn(&mut Marks),
) {
    let saved = builder.marks;
    mark(&mut builder.marks);
    walk(node, builder, picture, preformatted);
    builder.marks = saved;
}

/// Returns the pictures in `blocks`, including those inside quotes and
/// lists at any depth.
pub fn pictures(blocks: &[Block]) -> Vec<Arc<RenderImage>> {
    let mut found = Vec::new();
    for block in blocks {
        match block {
            Block::Picture(picture) => found.push(picture.image.clone()),
            Block::Quote(inner) => found.extend(pictures(inner)),
            Block::List { items, .. } => found.extend(items.iter().flat_map(|item| pictures(item))),
            _ => {}
        }
    }
    found
}

/// Returns the text of `blocks` without its styles. Tests use it to check
/// what a parsed document holds.
#[cfg(test)]
pub fn text_of(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Heading(_, inline) | Block::Paragraph(inline) => out.push_str(&inline.text),
            Block::Preformatted(text) => out.push_str(text),
            Block::Quote(inner) => out.push_str(&text_of(inner)),
            Block::List { items, .. } => {
                for item in items {
                    out.push_str(&text_of(item));
                    out.push('\n');
                }
            }
            Block::Table(rows) => {
                for row in rows {
                    for cell in row {
                        out.push_str(&cell.text);
                        out.push('\t');
                    }
                    out.push('\n');
                }
            }
            Block::Rule | Block::Picture(_) => {}
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Block, Inline, Marks, json, markdown, plain_text, text_of, xhtml};

    #[test]
    fn plain_text_keeps_every_line_in_pieces() {
        let text = (0..130)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>();
        let blocks = plain_text(&text.join("\r\n"));
        assert_eq!(blocks.len(), 3);
        let Block::Preformatted(first) = &blocks[0] else {
            panic!("preformatted")
        };
        assert_eq!(first.lines().count(), 64);
        assert!(text_of(&blocks).contains("line 129"));
    }

    #[test]
    fn json_is_pretty_printed_or_left_alone() {
        assert_eq!(
            json(r#"{"a":[1,2]}"#),
            vec![Block::Preformatted(
                "{\n  \"a\": [\n    1,\n    2\n  ]\n}".into()
            )]
        );
        assert_eq!(json("{nope"), vec![Block::Preformatted("{nope".into())]);
    }

    #[test]
    fn markdown_builds_headings_lists_quotes_and_code() {
        let blocks = markdown(
            "# Title\n\nSome *em* and **strong** and `code`.\n\n- one\n- two\n\n> quoted\n\n```\nlet x = 1;\n```\n\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
        );
        assert_eq!(blocks[0], Block::Heading(1, Inline::plain("Title")));
        let Block::Paragraph(paragraph) = &blocks[1] else {
            panic!("paragraph")
        };
        assert_eq!(paragraph.text, "Some em and strong and code.");
        assert_eq!(
            paragraph.marks,
            vec![
                (
                    5..7,
                    Marks {
                        italic: true,
                        ..Default::default()
                    }
                ),
                (
                    12..18,
                    Marks {
                        bold: true,
                        ..Default::default()
                    }
                ),
                (
                    23..27,
                    Marks {
                        code: true,
                        ..Default::default()
                    }
                ),
            ]
        );
        assert!(matches!(&blocks[2], Block::List { start: None, items } if items.len() == 2));
        assert!(matches!(&blocks[3], Block::Quote(inner) if inner.len() == 1));
        assert_eq!(blocks[4], Block::Preformatted("let x = 1;".into()));
        assert_eq!(blocks[5], Block::Rule);
        assert!(
            matches!(&blocks[6], Block::Table(rows) if rows.len() == 2 && rows[1][1].text == "2")
        );
    }

    #[test]
    fn xhtml_chapters_collapse_whitespace_and_keep_structure() {
        let chapter = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>x</title><style>p{}</style></head>
<body><h2>Chapter&nbsp;One</h2>
  <p>It was   a <em>dark</em>
     night.</p>
  <script>alert(1)</script>
  <img src="../images/a.png"/>
  <ol><li>first</li><li>second</li></ol>
</body></html>"#;
        let mut asked = Vec::new();
        let blocks = xhtml(chapter, &mut |source| {
            asked.push(source.to_owned());
            None
        });
        assert_eq!(asked, ["../images/a.png"]);
        assert_eq!(
            blocks[0],
            Block::Heading(2, Inline::plain("Chapter\u{a0}One"))
        );
        let Block::Paragraph(paragraph) = &blocks[1] else {
            panic!("paragraph")
        };
        assert_eq!(paragraph.text, "It was a dark night.");
        assert!(matches!(&blocks[2], Block::List { start: Some(1), items } if items.len() == 2));
        assert!(!text_of(&blocks).contains("alert"));
    }

    /// Returns the number of levels to which quotes and lists are nested in
    /// `blocks`.
    fn nesting(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| match block {
                Block::Quote(inner) => 1 + nesting(inner),
                Block::List { items, .. } => {
                    1 + items.iter().map(|item| nesting(item)).max().unwrap_or(0)
                }
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn nesting_without_end_is_flattened_and_keeps_the_text() {
        let quotes = format!("{} deep", ">".repeat(20_000));
        let blocks = markdown(&quotes);
        assert!(nesting(&blocks) <= 33, "{}", nesting(&blocks));
        assert!(text_of(&blocks).contains("deep"));
        let lists: String = (0..5_000)
            .map(|level| format!("{}- {level}\n", "  ".repeat(level)))
            .collect();
        let blocks = markdown(&lists);
        assert!(nesting(&blocks) <= 33);
        assert!(text_of(&blocks).contains("4999"));
        // A chapter whose elements are nested deeper than `MAX_DEPTH` keeps
        // its text, in the order of the file.
        let chapter = format!(
            "<html><body>{}<p>end</p>{}</body></html>",
            "<div><blockquote>".repeat(10_000),
            "</blockquote></div>".repeat(10_000)
        );
        let blocks = xhtml(
            &format!("{}x", chapter.replace("<p>end", "<p>start</p><p>end")),
            &mut |_| None,
        );
        let text = text_of(&blocks);
        assert!(text.contains("start") && text.contains("end"), "{text}");
        assert!(nesting(&blocks) <= 33);
    }
}
