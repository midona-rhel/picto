//! Text with CSS `letter-spacing`, which GPUI's text has no setting for: the
//! line is shaped once as usual and each glyph is then drawn moved along by
//! the spacing of every character before it. As in Chromium, the spacing
//! follows every character, the last one too, so it widens the box.
use gpui::*;

/// A single line of `text` in `font` at `size`, `spacing` points apart
/// beyond the font's own advances (`letter-spacing: <spacing>px`).
#[derive(IntoElement)]
pub struct Tracked {
    text: SharedString,
    font: Font,
    size: f32,
    line_height: f32,
    color: Hsla,
    spacing: f32,
}

pub fn tracked(
    text: impl Into<SharedString>,
    font: Font,
    size: f32,
    line_height: f32,
    color: impl Into<Hsla>,
    spacing: f32,
) -> Tracked {
    Tracked {
        text: text.into(),
        font,
        size,
        line_height,
        color: color.into(),
        spacing,
    }
}

/// How far the glyph of the `index`th character moves: the spacing of the
/// characters before it.
fn shift(index: usize, spacing: f32) -> f32 {
    index as f32 * spacing
}

impl RenderOnce for Tracked {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let run = TextRun {
            len: self.text.len(),
            font: self.font.clone(),
            color: self.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(self.text.clone(), px(self.size), &[run], None);
        let characters = self.text.chars().count();
        let width = f32::from(line.width) + shift(characters, self.spacing);
        // Byte offsets of the characters, to find each glyph's character.
        let starts = self
            .text
            .char_indices()
            .map(|(at, _)| at)
            .collect::<Vec<_>>();
        let (spacing, color, line_height) = (self.spacing, self.color, px(self.line_height));
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let padding = (line_height - line.ascent - line.descent) / 2.;
                let baseline = bounds.origin.y + padding + line.ascent;
                for run in &line.runs {
                    for glyph in &run.glyphs {
                        let character = starts.partition_point(|start| *start < glyph.index);
                        let origin = point(
                            bounds.origin.x + glyph.position.x + px(shift(character, spacing)),
                            baseline + glyph.position.y,
                        );
                        let _ = window.paint_glyph(
                            origin,
                            run.font_id,
                            glyph.id,
                            line.font_size,
                            color,
                        );
                    }
                }
            },
        )
        .flex_none()
        .w(px(width))
        .h(line_height)
    }
}

#[cfg(test)]
mod tests {
    use super::shift;

    #[test]
    fn each_character_moves_by_the_spacing_of_those_before_it() {
        assert_eq!(shift(0, 0.4), 0.);
        assert!((shift(8, 0.4) - 3.2).abs() < 1e-5);
        assert!((shift(3, -0.18) + 0.54).abs() < 1e-5);
    }
}
