//! Clipboard image helpers: Windows DIB <-> RGBA <-> PNG, plus thumbnails.
//! Pure functions (no Win32), so everything here is unit-tested on any OS.

use std::io::Cursor;

/// RGBA8 image, straight (non-premultiplied) alpha, top row first.
#[derive(Clone, Debug, PartialEq)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// Refuse absurd sizes (a 16k x 16k image would need 1 GB of RGBA).
pub const MAX_PIXELS: u64 = 64_000_000;

pub fn encode_png(img: &Rgba) -> Option<Vec<u8>> {
    if img.w == 0 || img.h == 0 || img.px.len() != (img.w as usize) * (img.h as usize) * 4 {
        return None;
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.w, img.h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().ok()?;
        w.write_image_data(&img.px).ok()?;
    }
    Some(out)
}

pub fn decode_png(bytes: &[u8]) -> Option<Rgba> {
    let mut dec = png::Decoder::new(Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    {
        // Refuse absurd dimensions *before* allocating the output buffer.
        let i = reader.info();
        if i.width == 0 || i.height == 0 || (i.width as u64) * (i.height as u64) > MAX_PIXELS {
            return None;
        }
    }
    let size = reader.output_buffer_size()?;
    let mut buf = vec![0u8; size];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_PIXELS {
        return None;
    }
    let data = &buf[..info.buffer_size()];
    let n = (w as usize) * (h as usize);
    let mut px = Vec::with_capacity(n * 4);
    match info.color_type {
        png::ColorType::Rgba => px.extend_from_slice(data.get(..n * 4)?),
        png::ColorType::Rgb => {
            for c in data.get(..n * 3)?.chunks_exact(3) {
                px.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in data.get(..n)? {
                px.extend_from_slice(&[g, g, g, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for c in data.get(..n * 2)?.chunks_exact(2) {
                px.extend_from_slice(&[c[0], c[0], c[0], c[1]]);
            }
        }
        png::ColorType::Indexed => return None, // expanded by the transformation; cannot happen
    }
    Some(Rgba { w, h, px })
}

/// Width/height from a PNG header without decoding it.
pub fn png_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if b.len() < 24 || b[..8] != SIG || &b[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
    let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
    (w > 0 && h > 0).then_some((w, h))
}

/// Box-filter downscale so that the longer side is at most `max` pixels.
pub fn thumbnail(img: &Rgba, max: u32) -> Rgba {
    let longest = img.w.max(img.h);
    if longest <= max || max == 0 {
        return img.clone();
    }
    let scale = max as f64 / longest as f64;
    let tw = ((img.w as f64 * scale).round() as u32).max(1);
    let th = ((img.h as f64 * scale).round() as u32).max(1);
    let mut px = vec![0u8; (tw as usize) * (th as usize) * 4];
    for ty in 0..th {
        let y0 = (ty as u64 * img.h as u64 / th as u64) as u32;
        let y1 = (((ty as u64 + 1) * img.h as u64 / th as u64) as u32)
            .max(y0 + 1)
            .min(img.h);
        for tx in 0..tw {
            let x0 = (tx as u64 * img.w as u64 / tw as u64) as u32;
            let x1 = (((tx as u64 + 1) * img.w as u64 / tw as u64) as u32)
                .max(x0 + 1)
                .min(img.w);
            let mut acc = [0u64; 4];
            let mut cnt = 0u64;
            for y in y0..y1 {
                let row = (y as usize) * (img.w as usize) * 4;
                for x in x0..x1 {
                    let i = row + (x as usize) * 4;
                    // Weight colour by alpha so transparent pixels do not darken the edges.
                    let a = img.px[i + 3] as u64;
                    acc[0] += img.px[i] as u64 * a;
                    acc[1] += img.px[i + 1] as u64 * a;
                    acc[2] += img.px[i + 2] as u64 * a;
                    acc[3] += a;
                    cnt += 1;
                }
            }
            let o = ((ty as usize) * (tw as usize) + tx as usize) * 4;
            if let Some(div) = std::num::NonZeroU64::new(acc[3]) {
                px[o] = (acc[0] / div) as u8;
                px[o + 1] = (acc[1] / div) as u8;
                px[o + 2] = (acc[2] / div) as u8;
            }
            px[o + 3] = (acc[3] / cnt.max(1)) as u8;
        }
    }
    Rgba { w: tw, h: th, px }
}

fn rd_u16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}
fn rd_u32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn rd_i32(b: &[u8], o: usize) -> Option<i32> {
    rd_u32(b, o).map(|v| v as i32)
}

/// Parse a packed DIB (what `CF_DIB` holds: BITMAPINFOHEADER/V4/V5 followed by pixels).
/// Supports 24-bit and 32-bit uncompressed bitmaps, which covers screenshots and "Copy image".
/// If a 32-bit bitmap has no alpha at all (all zeros — the usual case) it is treated as opaque.
pub fn dib_to_rgba(dib: &[u8]) -> Option<Rgba> {
    let hdr = rd_u32(dib, 0)? as usize;
    if !(40..=124).contains(&hdr) {
        return None; // BITMAPCOREHEADER and unknown headers are not supported
    }
    let width = rd_i32(dib, 4)?;
    let height = rd_i32(dib, 8)?;
    let bpp = rd_u16(dib, 14)?;
    let compression = rd_u32(dib, 16)?;
    let clr_used = rd_u32(dib, 32)? as usize;
    if width <= 0 || height == 0 || !(bpp == 24 || bpp == 32) {
        return None;
    }
    let top_down = height < 0;
    let (w, h) = (width as u32, height.unsigned_abs());
    if (w as u64) * (h as u64) > MAX_PIXELS {
        return None;
    }
    let mut off = hdr;
    match compression {
        0 => {}
        3 => {
            // BI_BITFIELDS: only the standard BGRA layout is supported.
            let (r, g, b) = if hdr == 40 {
                let m = (rd_u32(dib, 40)?, rd_u32(dib, 44)?, rd_u32(dib, 48)?);
                off += 12;
                m
            } else {
                (rd_u32(dib, 40)?, rd_u32(dib, 44)?, rd_u32(dib, 48)?)
            };
            if bpp != 32 || (r, g, b) != (0x00FF_0000, 0x0000_FF00, 0x0000_00FF) {
                return None;
            }
        }
        _ => return None,
    }
    off += clr_used * 4;
    let bytes_pp = (bpp / 8) as usize;
    let stride = ((w as usize) * (bpp as usize)).div_ceil(32) * 4;
    let need = off.checked_add(stride.checked_mul(h as usize)?)?;
    let data = dib.get(off..need)?;

    let mut px = vec![0u8; (w as usize) * (h as usize) * 4];
    let mut any_alpha = false;
    for row in 0..h as usize {
        let src_row = if top_down { row } else { h as usize - 1 - row };
        let src = &data[src_row * stride..src_row * stride + (w as usize) * bytes_pp];
        let dst = &mut px[row * (w as usize) * 4..(row + 1) * (w as usize) * 4];
        for (s, d) in src.chunks_exact(bytes_pp).zip(dst.chunks_exact_mut(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = if bytes_pp == 4 { s[3] } else { 255 };
            any_alpha |= d[3] != 0;
        }
    }
    if !any_alpha {
        for d in px.chunks_exact_mut(4) {
            d[3] = 255;
        }
    }
    Some(Rgba { w, h, px })
}

/// Build a packed 32-bit bottom-up DIB for `CF_DIB`. Transparent areas are composited on white,
/// because many programs ignore DIB alpha (the PNG clipboard format keeps the real alpha).
pub fn rgba_to_dib(img: &Rgba) -> Vec<u8> {
    let (w, h) = (img.w as usize, img.h as usize);
    let mut out = Vec::with_capacity(40 + w * h * 4);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(img.w as i32).to_le_bytes());
    out.extend_from_slice(&(img.h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&((w * h * 4) as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // resolution + palette counts
    for row in (0..h).rev() {
        for p in img.px[row * w * 4..(row + 1) * w * 4].chunks_exact(4) {
            let a = p[3] as u32;
            let blend = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
            out.extend_from_slice(&[blend(p[2]), blend(p[1]), blend(p[0]), 255]);
        }
    }
    out
}

pub fn human_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    let f = n as f64;
    if f < KB {
        format!("{n} B")
    } else if f < KB * KB {
        format!("{:.0} KB", f / KB)
    } else {
        format!("{:.1} MB", f / (KB * KB))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Rgba {
        // 3x2: red, green, blue / white, black, translucent grey
        let px = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, //
            255, 255, 255, 255, 0, 0, 0, 255, 128, 128, 128, 100,
        ];
        Rgba { w: 3, h: 2, px }
    }

    #[test]
    fn png_roundtrip_keeps_alpha() {
        let img = sample();
        let png = encode_png(&img).unwrap();
        assert_eq!(png_dimensions(&png), Some((3, 2)));
        assert_eq!(decode_png(&png).unwrap(), img);
    }

    #[test]
    fn bad_png_is_rejected() {
        assert!(decode_png(b"not a png").is_none());
        assert!(png_dimensions(b"short").is_none());
        assert!(encode_png(&Rgba {
            w: 2,
            h: 2,
            px: vec![0; 3]
        })
        .is_none());
    }

    #[test]
    fn dib_32bit_bottom_up_roundtrip() {
        let img = Rgba {
            px: sample()
                .px
                .chunks_exact(4)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            ..sample()
        };
        let dib = rgba_to_dib(&img);
        assert_eq!(dib_to_rgba(&dib).unwrap(), img);
    }

    #[test]
    fn dib_24bit_with_row_padding_and_top_down() {
        // 1 pixel wide, 2 rows, 24 bpp => each row is 3 bytes + 1 byte padding.
        let mut d = Vec::new();
        d.extend_from_slice(&40u32.to_le_bytes());
        d.extend_from_slice(&1i32.to_le_bytes());
        d.extend_from_slice(&(-2i32).to_le_bytes()); // top-down
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&24u16.to_le_bytes());
        d.extend_from_slice(&[0u8; 24]);
        d.extend_from_slice(&[0, 0, 255, 0]); // row 0: BGR = red
        d.extend_from_slice(&[255, 0, 0, 0]); // row 1: BGR = blue
        let img = dib_to_rgba(&d).unwrap();
        assert_eq!((img.w, img.h), (1, 2));
        assert_eq!(&img.px[..4], &[255, 0, 0, 255]);
        assert_eq!(&img.px[4..], &[0, 0, 255, 255]);
    }

    #[test]
    fn dib_without_alpha_becomes_opaque() {
        let mut d = Vec::new();
        d.extend_from_slice(&40u32.to_le_bytes());
        d.extend_from_slice(&1i32.to_le_bytes());
        d.extend_from_slice(&1i32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&32u16.to_le_bytes());
        d.extend_from_slice(&[0u8; 24]);
        d.extend_from_slice(&[10, 20, 30, 0]); // BGRA with alpha 0
        assert_eq!(dib_to_rgba(&d).unwrap().px, vec![30, 20, 10, 255]);
    }

    #[test]
    fn unsupported_or_truncated_dibs_fail_cleanly() {
        assert!(dib_to_rgba(&[1, 2, 3]).is_none());
        let mut d = Vec::new();
        d.extend_from_slice(&40u32.to_le_bytes());
        d.extend_from_slice(&100i32.to_le_bytes());
        d.extend_from_slice(&100i32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&32u16.to_le_bytes());
        d.extend_from_slice(&[0u8; 24]);
        assert!(dib_to_rgba(&d).is_none()); // pixel data missing
        d[14] = 8; // 8 bpp (palettised)
        assert!(dib_to_rgba(&d).is_none());
    }

    #[test]
    fn thumbnail_keeps_aspect_and_averages() {
        let img = Rgba {
            w: 200,
            h: 100,
            px: [10u8, 20, 30, 255].repeat(200 * 100),
        };
        let t = thumbnail(&img, 50);
        assert_eq!((t.w, t.h), (50, 25));
        assert_eq!(&t.px[..4], &[10, 20, 30, 255]);
        // Already small: unchanged.
        assert_eq!(thumbnail(&sample(), 50), sample());
    }

    #[test]
    fn bytes_formatting() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2 KB");
        assert_eq!(human_bytes(3 * 1024 * 1024 + 512 * 1024), "3.5 MB");
    }
}

#[cfg(test)]
mod perf {
    use super::*;

    /// Rough timing of the background image pipeline on a 4K screenshot-like frame.
    /// Run with: cargo test --release perf -- --nocapture
    #[test]
    fn decode_png_rejects_absurd_dimensions_without_allocating() {
        // A valid PNG header claiming 60000 x 60000 pixels (14 GB of RGBA); must be refused early.
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&60_000u32.to_be_bytes());
        ihdr.extend_from_slice(&60_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        png.extend_from_slice(&13u32.to_be_bytes());
        let mut chunk = b"IHDR".to_vec();
        chunk.extend_from_slice(&ihdr);
        let crc = {
            let mut c = 0xFFFF_FFFFu32;
            for &b in &chunk {
                c ^= b as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        };
        png.extend_from_slice(&chunk);
        png.extend_from_slice(&crc.to_be_bytes());
        assert!(decode_png(&png).is_none());
    }

    #[test]
    fn four_k_pipeline_is_fast_enough() {
        let (w, h) = (3840u32, 2160u32);
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[
                    (x % 251) as u8,
                    (y % 241) as u8,
                    ((x + y) % 239) as u8,
                    255,
                ]);
            }
        }
        let img = Rgba { w, h, px };
        let dib = rgba_to_dib(&img);
        let t = std::time::Instant::now();
        let parsed = dib_to_rgba(&dib).unwrap();
        let png = encode_png(&parsed).unwrap();
        let thumb = encode_png(&thumbnail(&parsed, 128)).unwrap();
        let ms = t.elapsed().as_millis();
        println!(
            "4K: dib->rgba->png {} KB, thumb {} B, {} ms",
            png.len() / 1024,
            thumb.len(),
            ms
        );
        if !cfg!(debug_assertions) {
            assert!(ms < 5_000); // debug builds are far slower; only judge release
        }
    }
}
