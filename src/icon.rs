//! Procedurally drawn app icon (magnifier on a rounded gradient square) – no image assets needed.

pub fn rgba(size: u32) -> Vec<u8> {
    let n = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let corner = n * 0.22;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded-square coverage via signed distance.
            let (cx, cy) = (
                (fx - n / 2.0).abs() - (n / 2.0 - corner),
                (fy - n / 2.0).abs() - (n / 2.0 - corner),
            );
            let d =
                (cx.max(0.0).powi(2) + cy.max(0.0).powi(2)).sqrt() + cx.max(cy).min(0.0) - corner;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            // Diagonal gradient blue → violet.
            let t = (fx + fy) / (2.0 * n);
            let (mut r, mut g, mut b) = (
                lerp(66.0, 124.0, t),
                lerp(133.0, 92.0, t),
                lerp(255.0, 255.0, t),
            );
            // Magnifier lens ring.
            let (lx, ly, lr) = (n * 0.44, n * 0.44, n * 0.23);
            let dl = ((fx - lx).powi(2) + (fy - ly).powi(2)).sqrt();
            let ring = 1.0 - ((dl - lr).abs() - n * 0.045).clamp(0.0, 1.0);
            // Handle: distance to segment.
            let (ax, ay, bx, by) = (n * 0.61, n * 0.61, n * 0.80, n * 0.80);
            let (px_, py_) = (fx - ax, fy - ay);
            let (vx, vy) = (bx - ax, by - ay);
            let h = ((px_ * vx + py_ * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
            let dh = ((px_ - vx * h).powi(2) + (py_ - vy * h).powi(2)).sqrt();
            let handle = 1.0 - (dh - n * 0.05).clamp(0.0, 1.0);
            let w = ring.max(handle).clamp(0.0, 1.0);
            r = lerp(r, 255.0, w);
            g = lerp(g, 255.0, w);
            b = lerp(b, 255.0, w);
            let i = ((y * size + x) * 4) as usize;
            px[i] = r as u8;
            px[i + 1] = g as u8;
            px[i + 2] = b as u8;
            px[i + 3] = (cover * 255.0) as u8;
        }
    }
    px
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_has_expected_size_and_transparent_corner() {
        let v = super::rgba(32);
        assert_eq!(v.len(), 32 * 32 * 4);
        assert_eq!(v[3], 0); // top-left corner is transparent
        assert!(v[(16 * 32 + 16) * 4 + 3] > 200); // centre is opaque
    }
}
