use super::*;

/// 0.0–1.0 colour to the layered panel's RGB and alpha.
pub fn panel_color(rgba: [f32; 4]) -> ((u8, u8, u8), u8) {
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    ((byte(rgba[0]), byte(rgba[1]), byte(rgba[2])), byte(rgba[3]))
}

/// 0.0–1.0 Box body RGBA + blend fade -> header titleband RGB and readable alpha.
pub fn header_color(rgba: [f32; 4], fade: f32) -> ((u8, u8, u8), u8) {
    let fade = fade.clamp(0., 1.);
    let bg_a = (rgba[3].clamp(0., 1.) * fade).clamp(0., 1.);
    let band_a = (0.20 * fade).clamp(0., 1.);
    let out_a = 1.0 - (1.0 - bg_a) * (1.0 - band_a);
    let scale = if out_a > 1e-4 { ((1.0 - band_a) * bg_a) / out_a } else { 0.0 };
    let chan = |c: f32| (c.clamp(0., 1.) * scale * 255.).round().clamp(0., 255.) as u8;
    let min_a = if fade <= 0.05 { 0.0 } else { 115.0 };
    let alpha = (out_a * 255.).round().clamp(min_a, 255.) as u8;
    ((chan(rgba[0]), chan(rgba[1]), chan(rgba[2])), alpha)
}
