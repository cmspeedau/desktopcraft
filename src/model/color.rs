use super::*;

/// Hue (0..1), saturation (0..1), brightness (0..1) to red, green, blue.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1. - s), v * (1. - s * f), v * (1. - s * (1. - f)));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}
/// Red, green, blue to hue, saturation, brightness (inverse of hsv_to_rgb).
pub fn rgb_to_hsv(c: [f32; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (c[0], c[1], c[2]);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0. {
        0.
    } else if max == r {
        ((g - b) / d).rem_euclid(6.) / 6.
    } else if max == g {
        ((b - r) / d + 2.) / 6.
    } else {
        ((r - g) / d + 4.) / 6.
    };
    (h, if max == 0. { 0. } else { d / max }, max)
}
