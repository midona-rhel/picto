//! A palette colour written out the ways the swatch menu copies it
//! (`colorCopyEntries` in `ColorPalette.tsx`).

fn channels(color: u32) -> [u8; 3] {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8]
}

/// `labToHex`: `#rrggbb` in lower case.
pub fn hex(color: u32) -> String {
    format!("#{:06x}", color & 0xff_ffff)
}

pub fn rgb(color: u32) -> String {
    let [r, g, b] = channels(color);
    format!("rgb({r}, {g}, {b})")
}

pub fn rgba(color: u32) -> String {
    let [r, g, b] = channels(color);
    format!("rgba({r}, {g}, {b}, 1)")
}

fn unit(color: u32) -> [f32; 3] {
    channels(color).map(|channel| channel as f32 / 255.)
}

/// JavaScript's `Math.round`: halves go up.
fn round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

pub fn hsl(color: u32) -> String {
    let [r, g, b] = unit(color);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.;
    if max == min {
        return format!("hsl(0, 0%, {}%)", round(lightness * 100.));
    }
    let delta = max - min;
    let saturation = if lightness > 0.5 {
        delta / (2. - max - min)
    } else {
        delta / (max + min)
    };
    let hue = if max == r {
        ((g - b) / delta + if g < b { 6. } else { 0. }) / 6.
    } else if max == g {
        ((b - r) / delta + 2.) / 6.
    } else {
        ((r - g) / delta + 4.) / 6.
    };
    format!(
        "hsl({}, {}%, {}%)",
        round(hue * 360.),
        round(saturation * 100.),
        round(lightness * 100.)
    )
}

/// The hue of `rgbToHsv`, in whole degrees.
fn hsv_hue(color: u32) -> i32 {
    let [r, g, b] = unit(color);
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    let hue = if delta == 0. {
        0.
    } else if max == r {
        // JavaScript's `%` keeps the sign of the dividend, as Rust's does.
        60. * (((g - b) / delta) % 6.)
    } else if max == g {
        60. * ((b - r) / delta + 2.)
    } else {
        60. * ((r - g) / delta + 4.)
    };
    round(if hue < 0. { hue + 360. } else { hue })
}

pub fn hsv(color: u32) -> String {
    let [r, g, b] = unit(color);
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    let saturation = if max == 0. { 0. } else { delta / max };
    format!(
        "hsv({}, {}%, {}%)",
        hsv_hue(color),
        round(saturation * 100.),
        round(max * 100.)
    )
}

pub fn hwb(color: u32) -> String {
    let [r, g, b] = unit(color);
    format!(
        "hwb({} {}% {}%)",
        hsv_hue(color),
        round(r.min(g).min(b) * 100.),
        round((1. - r.max(g).max(b)) * 100.)
    )
}

pub fn cmyk(color: u32) -> String {
    let [r, g, b] = unit(color);
    let key = 1. - r.max(g).max(b);
    if key == 1. {
        return "cmyk(0%, 0%, 0%, 100%)".to_owned();
    }
    let component = |value: f32| round((1. - value - key) / (1. - key) * 100.);
    format!(
        "cmyk({}%, {}%, {}%, {}%)",
        component(r),
        component(g),
        component(b),
        round(key * 100.)
    )
}

#[cfg(test)]
mod tests {
    use super::{cmyk, hex, hsl, hsv, hwb, rgb, rgba};

    #[test]
    fn a_colour_is_written_as_each_copy_row_writes_it() {
        let pink = 0xf0_6292;
        assert_eq!(hex(pink), "#f06292");
        assert_eq!(rgb(pink), "rgb(240, 98, 146)");
        assert_eq!(rgba(pink), "rgba(240, 98, 146, 1)");
        assert_eq!(hsl(pink), "hsl(340, 83%, 66%)");
        assert_eq!(hsv(pink), "hsv(340, 59%, 94%)");
        assert_eq!(hwb(pink), "hwb(340 38% 6%)");
        assert_eq!(cmyk(pink), "cmyk(0%, 59%, 39%, 6%)");
    }

    #[test]
    fn grey_and_black_have_no_hue() {
        assert_eq!(hsl(0x80_8080), "hsl(0, 0%, 50%)");
        assert_eq!(cmyk(0), "cmyk(0%, 0%, 0%, 100%)");
    }
}
