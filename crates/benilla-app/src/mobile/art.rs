//! The mobile interface's round control border, sharp on a phone's screen. The art is the stock
//! minimap button ring (`MiniMap-TrackingBorder`, a 38-texel ring in the corner of a 64-texel
//! square, magnified to a smear at three pixels a point), resampled once at startup from the
//! player's own install: cut out about its hole's centre, Lanczos-resized to [`SIZE`], sharpened,
//! and its silhouette tightened. [`register`] makes the result an interface texture by name
//! (`assets/mobile` sets it like any other), so the controls keep 1.12's look.

use image::{imageops, RgbaImage};

/// The ring, its hole at the texture's centre.
pub(crate) const RING: &str = "Interface\\WoWMobile\\Ring";

/// The stock ring the texture is made from.
const SOURCE: &str = "Interface\\Minimap\\MiniMap-TrackingBorder.blp";
/// Texels of the source kept on each side of the hole's centre: the ring and its drop shadow.
const HALF: u32 = 22;
const SIZE: u32 = 256;

/// Register the ring, once a process. Without an install the controls draw no ring.
pub(crate) fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| match source().and_then(|src| ring(&src)) {
        Some(px) => benilla_assets::register_generated_sprite(RING, SIZE, SIZE, px.into_raw()),
        None => bevy::log::warn!("mobile: no {SOURCE} to make the control ring from"),
    });
}

fn source() -> Option<RgbaImage> {
    let data = benilla_formats::wow_data()?;
    let mut chain = benilla_formats::open_chain(&data).ok()?;
    let bytes = chain.read_file(SOURCE).ok()?;
    let (w, h, px) = benilla_formats::blp_to_rgba(&bytes).ok()?;
    RgbaImage::from_raw(w, h, px)
}

/// The centre of the ring's hole: the clear texels the band encloses, which the drop shadow does
/// not pull off centre as it does the alpha's.
fn hole_centre(src: &RgbaImage) -> Option<(f32, f32)> {
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for y in 6..34 {
        for x in 6..34 {
            if src.get_pixel(x, y)[3] < 24 {
                sx += x as f32 + 0.5;
                sy += y as f32 + 0.5;
                n += 1.0;
            }
        }
    }
    (n > 0.0).then(|| (sx / n, sy / n))
}

fn ring(src: &RgbaImage) -> Option<RgbaImage> {
    let (cx, cy) = hole_centre(src)?;
    // The square about the hole, clear where it runs past the source's edge; premultiplied, as
    // Lanczos rings at a hard alpha edge on straight colour.
    let (x0, y0) = ((cx - HALF as f32).round() as i64, (cy - HALF as f32).round() as i64);
    let cut = RgbaImage::from_fn(HALF * 2, HALF * 2, |x, y| {
        let (sx, sy) = (x0 + i64::from(x), y0 + i64::from(y));
        if sx < 0 || sy < 0 || sx >= i64::from(src.width()) || sy >= i64::from(src.height()) {
            return image::Rgba([0, 0, 0, 0]);
        }
        let mut p = *src.get_pixel(sx as u32, sy as u32);
        let a = p[3] as f32 / 255.0;
        for c in 0..3 {
            p[c] = (p[c] as f32 * a).round() as u8;
        }
        p
    });
    let big = imageops::resize(&cut, SIZE, SIZE, imageops::FilterType::Lanczos3);
    let mut out = imageops::unsharpen(&big, 1.2, 4);
    for p in out.pixels_mut() {
        // Straight colour again, then the silhouette's soft ramp tightened.
        let a = p[3] as f32 / 255.0;
        for c in 0..3 {
            let straight = if a > 0.0 { p[c] as f32 / a } else { 0.0 };
            p[c] = straight.clamp(0.0, 255.0).round() as u8;
        }
        let tight = (a * 1.35 - 0.12).clamp(0.0, 1.0);
        p[3] = (tight * tight * (3.0 - 2.0 * tight) * 255.0).round() as u8;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_is_cut_about_its_hole() {
        benilla_formats::wow_data_or_skip!();
        let src = source().expect("the stock ring");
        let (cx, cy) = hole_centre(&src).unwrap();
        // The ring sits in the source's top-left corner.
        assert!((15.0..25.0).contains(&cx) && (15.0..25.0).contains(&cy), "{cx},{cy}");
        let px = ring(&src).unwrap();
        assert_eq!(px.dimensions(), (SIZE, SIZE));
        assert!(px.get_pixel(SIZE / 2, SIZE / 2)[3] < 16, "the hole is clear");
        assert_eq!(px.get_pixel(0, 0)[3], 0, "a corner is clear");
    }
}
