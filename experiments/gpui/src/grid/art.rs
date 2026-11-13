//! The artwork drawn in place of a thumbnail: the torn page of a thumbnail
//! that failed to load (`drawBrokenThumbnail.ts`) and the lettered card of a
//! font (`drawFontThumbnail.ts`). Each is an SVG built once for the theme's
//! placeholder colour and rasterised by GPUI's SVG renderer.
use gpui::*;
use std::sync::Arc;

/// The drawing's own size in `brokenThumbnailGeometry.ts` units.
pub const BROKEN_SIZE: (f32, f32) = (160., 176.);
/// `FONT_THUMBNAIL_SIZE`
pub const FONT_SIZE: f32 = 160.;
/// Room kept around each drawing for its shadow, in drawing units.
pub const MARGIN: f32 = 16.;

pub struct Art {
    color: Rgba,
    pub broken: Arc<Image>,
    pub font: Arc<Image>,
}

impl Art {
    /// The artwork for a placeholder colour, reused while it stays the same.
    pub fn for_color(cached: &mut Option<Art>, color: Rgba) -> &Art {
        if cached.as_ref().is_none_or(|art| art.color != color) {
            let hex = hex(color);
            *cached = Some(Art {
                color,
                broken: Arc::new(Image::from_bytes(
                    ImageFormat::Svg,
                    broken_svg(&hex).into_bytes(),
                )),
                font: Arc::new(Image::from_bytes(
                    ImageFormat::Svg,
                    font_svg(&hex).into_bytes(),
                )),
            });
        }
        cached.as_ref().expect("set above")
    }
}

/// `#rrggbb` for an opaque colour.
fn hex(color: Rgba) -> String {
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

/// A drawing of `width` by `height` units with `MARGIN` around it, rendered
/// at twice its size so it stays sharp on a Retina display.
fn document(width: f32, height: f32, body: &str) -> String {
    let (outer_width, outer_height) = (width + 2. * MARGIN, height + 2. * MARGIN);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" \
         viewBox=\"{m} {m} {outer_width} {outer_height}\">{body}</svg>",
        w = outer_width * 2.,
        h = outer_height * 2.,
        m = -MARGIN,
    )
}

/// `drawBrokenThumbnail`: a page with a folded corner, lit from the top left,
/// cut through by the placeholder colour along its fold and a crack.
/// The canvas shadow (blur 9, 6px down, 36% black) is a Gaussian of half the
/// blur.
fn broken_svg(background: &str) -> String {
    let (width, height) = BROKEN_SIZE;
    document(
        width,
        height,
        &format!(
            "<defs>\
             <linearGradient id=\"paper\" x1=\"45\" y1=\"18\" x2=\"112\" y2=\"160\" gradientUnits=\"userSpaceOnUse\">\
             <stop offset=\"0\" stop-color=\"#f7f8fa\" stop-opacity=\".9\"/>\
             <stop offset=\"1\" stop-color=\"#a7abb2\" stop-opacity=\".66\"/>\
             </linearGradient>\
             <linearGradient id=\"fold\" x1=\"108\" y1=\"18\" x2=\"137\" y2=\"47\" gradientUnits=\"userSpaceOnUse\">\
             <stop offset=\"0\" stop-color=\"#eceef1\" stop-opacity=\".86\"/>\
             <stop offset=\"1\" stop-color=\"#9da2aa\" stop-opacity=\".62\"/>\
             </linearGradient>\
             <filter id=\"shadow\" x=\"-16\" y=\"-16\" width=\"192\" height=\"208\" filterUnits=\"userSpaceOnUse\">\
             <feDropShadow dx=\"0\" dy=\"6\" stdDeviation=\"4.5\" flood-color=\"#000\" flood-opacity=\".36\"/>\
             </filter>\
             </defs>\
             <path d=\"M31 18H108L137 47V151C137 157 132 160 126 160H31C26 160 23 156 23 151V27C23 22 26 18 31 18Z\" fill=\"url(#paper)\" filter=\"url(#shadow)\"/>\
             <path d=\"M108 18L137 47H108Z\" fill=\"url(#fold)\"/>\
             <g fill=\"none\" stroke=\"{background}\" stroke-width=\"11\" stroke-linejoin=\"miter\" stroke-linecap=\"butt\">\
             <path d=\"M108 18V47H137\"/>\
             <path d=\"M20 104L76 71V113L140 79\"/>\
             </g>"
        ),
    )
}

/// `drawFontThumbnail`: a rounded card with "Aa" punched through it. The
/// canvas punches the letters out and repaints them in the placeholder
/// colour, so here they are drawn in that colour over the card. Baskerville
/// bold, 124 and 84 units, 4 units closer than their advance, on the
/// baseline `FONT_THUMBNAIL_BASELINE`.
fn font_svg(background: &str) -> String {
    document(
        FONT_SIZE,
        FONT_SIZE,
        &format!(
            "<defs>\
             <linearGradient id=\"card\" x1=\"30\" y1=\"12\" x2=\"130\" y2=\"152\" gradientUnits=\"userSpaceOnUse\">\
             <stop offset=\"0\" stop-color=\"#f7f8fa\" stop-opacity=\".88\"/>\
             <stop offset=\"1\" stop-color=\"#a7abb2\" stop-opacity=\".68\"/>\
             </linearGradient>\
             <filter id=\"shadow\" x=\"-16\" y=\"-16\" width=\"192\" height=\"192\" filterUnits=\"userSpaceOnUse\">\
             <feDropShadow dx=\"0\" dy=\"5\" stdDeviation=\"3\" flood-color=\"#000\" flood-opacity=\".24\"/>\
             </filter>\
             </defs>\
             <rect x=\"8\" y=\"8\" width=\"144\" height=\"144\" rx=\"12\" fill=\"url(#card)\" filter=\"url(#shadow)\"/>\
             <text x=\"80\" y=\"122\" text-anchor=\"middle\" fill=\"{background}\" \
             font-family=\"Baskerville, Georgia, 'Times New Roman', serif\" font-weight=\"700\">\
             <tspan font-size=\"124\">A</tspan><tspan font-size=\"84\" dx=\"-4\">a</tspan>\
             </text>"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::{broken_svg, font_svg, hex};
    use gpui::rgb;

    #[test]
    fn the_cut_lines_take_the_placeholder_colour() {
        assert_eq!(hex(rgb(0x232427)), "#232427");
        assert!(broken_svg("#232427").contains("stroke=\"#232427\""));
        assert!(font_svg("#232427").contains("fill=\"#232427\""));
    }

    /// The pixel at drawing coordinates (`x`, `y`), BGRA.
    fn pixel(svg: &str, x: f32, y: f32) -> [u8; 4] {
        let renderer = gpui::SvgRenderer::new(std::sync::Arc::new(()));
        let image = renderer
            .render_single_frame(svg.as_bytes(), 1.)
            .expect("the SVG renders");
        let size = image.size(0);
        let width = i32::from(size.width) as usize;
        // Rendered at twice the document's size, itself twice the drawing's.
        let scale = width as f32 / (super::FONT_SIZE + 2. * super::MARGIN);
        let column = ((x + super::MARGIN) * scale) as usize;
        let row = ((y + super::MARGIN) * scale) as usize;
        let at = (row * width + column) * 4;
        image.as_bytes(0).unwrap()[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn the_artwork_renders_with_its_cuts_and_letters() {
        // The page's paper is light; the crack through it is the placeholder.
        let broken = broken_svg("#ff0000");
        let paper = pixel(&broken, 40., 140.);
        assert!(paper[3] > 150 && paper[0] > 150, "paper: {paper:?}");
        let crack = pixel(&broken, 76., 90.);
        assert_eq!(
            (crack[2], crack[1], crack[0]),
            (255, 0, 0),
            "crack: {crack:?}"
        );
        // The card is light around the letters; inside the A's left stroke
        // the placeholder shows.
        let font = font_svg("#ff0000");
        let card = pixel(&font, 20., 20.);
        assert!(card[3] > 150 && card[0] > 150, "card: {card:?}");
        let letters = (30..130)
            .filter(|x| {
                let p = pixel(&font, *x as f32, 110.);
                p[2] > 200 && p[1] < 60 && p[0] < 60
            })
            .count();
        assert!(letters > 10, "no letters drawn");
    }

    #[test]
    fn the_drawings_leave_room_for_their_shadows() {
        let broken = broken_svg("#000000");
        assert!(broken.contains("viewBox=\"-16 -16 192 208\""));
        assert!(broken.contains("width=\"384\" height=\"416\""));
        assert!(font_svg("#000000").contains("viewBox=\"-16 -16 192 192\""));
    }
}
