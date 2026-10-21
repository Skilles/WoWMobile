//! The iOS app icon, made at build time from the player's own install: the glue screen's logo
//! (`Glues-WoW-Logo.blp`) centred on a dark square, opaque, 1024x1024, as a PNG. The output is
//! derived from the install, so it lives only in the gitignored build tree.
//! `cargo run -p benilla-formats --example ios_icon -- <out.png>`

use image::{imageops, Rgba, RgbaImage};

const SIZE: u32 = 1024;
const LOGO: &str = "Interface\\Glues\\Common\\Glues-WoW-Logo.blp";

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: ios_icon <out.png>"))?;
    let data = benilla_formats::wow_data().expect("no WoW install found (set $WOW_DATA)");
    let mut chain = benilla_formats::open_chain(&data)?;
    let blp = benilla_blp::decode(&chain.read_file(LOGO)?)?;
    let mip = &blp.mips[0];
    let logo = RgbaImage::from_raw(mip.width, mip.height, mip.rgba.clone())
        .ok_or_else(|| anyhow::anyhow!("logo mip 0 is not {}x{} RGBA", mip.width, mip.height))?;

    // A dark, warm vertical gradient, the glue sky's colours.
    let mut icon = RgbaImage::from_fn(SIZE, SIZE, |_, y| {
        let t = y as f32 / (SIZE - 1) as f32;
        let mix = |a: f32, b: f32| (a + (b - a) * t) as u8;
        Rgba([mix(12.0, 58.0), mix(8.0, 18.0), mix(20.0, 8.0), 255])
    });

    // The logo at 90% of the width, centred.
    let w = SIZE * 9 / 10;
    let h = w * mip.height / mip.width;
    let scaled = imageops::resize(&logo, w, h, imageops::FilterType::Lanczos3);
    imageops::overlay(
        &mut icon,
        &scaled,
        i64::from((SIZE - w) / 2),
        i64::from((SIZE - h) / 2),
    );

    // iOS rejects an icon with an alpha channel: write RGB.
    image::DynamicImage::ImageRgba8(icon).to_rgb8().save(&out)?;
    println!("{LOGO} ({}x{}) -> {out}", mip.width, mip.height);
    Ok(())
}
