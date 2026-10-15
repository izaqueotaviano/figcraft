//! The eyedropper picks exactly what the canvas shows under the pointer, in the file's colour
//! space: the canvas's own device pixel (at the screen's pixel density, in pixel preview, at
//! zoomed-out levels of detail), with what background blurs and glass show, and a solid fill as
//! it is stored.

use base64::Engine as _;
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use crate::tools::{PointerEvent, PointerKind};
use figcraft_color::{Color, Paint};
use figcraft_doc::NodeId;

fn id(v: Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

fn rect(s: &mut Session, (x, y, w, h): (f64, f64, f64, f64), fill: &str) -> NodeId {
    id(s.execute("shape.rectangle", &json!({"x": x, "y": y, "width": w, "height": h, "props": {"fill": fill}})).unwrap())
}

/// Pick at page point (x, y) with the canvas at `zoom` (nothing selected: only the pick).
fn pick(s: &mut Session, x: f64, y: f64, zoom: f64) -> Color {
    s.execute("select.set", &json!({"ids": []})).unwrap();
    s.execute("edit.pickColor", &json!({})).unwrap();
    s.pointer(&PointerEvent::at(PointerKind::Down, x, y, zoom)).unwrap();
    *s.recent_colors.first().unwrap()
}

/// The pixel the canvas shows at page point (x, y) at `zoom`: a render of a screenful around it the
/// way the canvas asks for one (on the device grid at the screen's density or the pixel preview
/// resolution, level of detail on).
fn shown(s: &Session, x: f64, y: f64, zoom: f64) -> [u8; 3] {
    shown_in(s, x, y, zoom, (173, 131, 400, 300))
}

/// [`shown`] in a render `w` × `h` device pixels whose pixel (`at_x`, `at_y`) is the one at (x, y).
fn shown_in(s: &Session, x: f64, y: f64, zoom: f64, (at_x, at_y, w, h): (u32, u32, u32, u32)) -> [u8; 3] {
    let ds = s.ds().unwrap();
    let scale = ds.view.render_scale(zoom);
    let (ox, oy) = (((x * scale).floor() - f64::from(at_x)) / scale, ((y * scale).floor() - f64::from(at_y)) / scale);
    let view = figcraft_render::View { origin: (ox, oy), scale, width: w, height: h };
    let opts = figcraft_render::Options { lod: true, ..Default::default() };
    let pix = figcraft_render::render_view(&ds.doc, ds.page, &view, &opts).unwrap();
    let c = pix.pixel(at_x, at_y).unwrap();
    assert_eq!(c.alpha(), 255, "an opaque page");
    [c.red(), c.green(), c.blue()]
}

fn rgb(c: Color) -> [u8; 3] {
    let [r, g, b, _] = c.to_rgba8();
    [r, g, b]
}

/// Orange and dark brown stripes 10 units wide.
fn stripes(s: &mut Session) {
    for i in 0..20 {
        rect(s, (f64::from(i) * 10.0, 0.0, 10.0, 200.0), if i % 2 == 0 { "#E67D30" } else { "#221005" });
    }
}

#[test]
fn background_blur_and_glass_are_picked_as_the_canvas_shows_them() {
    // A lone pixel's backdrop is only itself: the pick showed the stripes unblurred through the
    // panel (#F3BE98 where the canvas shows #C2A28C).
    for effect in ["backgroundBlur", "glass"] {
        let mut s = Session::new();
        stripes(&mut s);
        let panel = rect(&mut s, (40.0, 40.0, 120.0, 120.0), "#FFFFFF80");
        s.execute("effect.add", &json!({"ids": [panel], "type": effect})).unwrap();
        if effect == "backgroundBlur" {
            s.execute("effect.set", &json!({"ids": [panel], "index": 0, "effect": {"type": effect, "radius": 20}})).unwrap();
        }
        for (x, y) in [(100.5, 100.5), (73.2, 99.9), (45.0, 45.0), (165.0, 100.0)] {
            for zoom in [0.75, 1.0, 2.0, 5.0] {
                // A blur is approximated by box passes over what the render covers: a window's
                // and a screen's differ by a step at most.
                let (got, want) = (rgb(pick(&mut s, x, y, zoom)), shown(&s, x, y, zoom));
                assert!(got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 1), "{effect} at ({x}, {y}), zoom {zoom}: picked {got:?}, shown {want:?}");
            }
        }
    }
}

#[test]
fn a_huge_blur_far_zoomed_in_is_still_picked_through() {
    let mut s = Session::new();
    stripes(&mut s);
    let panel = rect(&mut s, (0.0, 0.0, 200.0, 200.0), "#FFFFFF00");
    s.execute("effect.add", &json!({"ids": [panel], "type": "backgroundBlur"})).unwrap();
    s.execute("effect.set", &json!({"ids": [panel], "index": 0, "effect": {"type": "backgroundBlur", "radius": 200}})).unwrap();
    // Its backdrop is too far to render at this zoom: it is sampled where it fits, and a blur
    // that wide is smooth (what a view of all it reads shows at 100%, within a few steps).
    let (got, want) = (rgb(pick(&mut s, 100.0, 100.0, 64.0)), shown_in(&s, 100.0, 100.0, 1.0, (700, 700, 1401, 1401)));
    assert!(got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 4), "picked {got:?}, shown {want:?}");
}

/// A picture of noise: each pixel its own colour, as in a photo.
fn noise_png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let v = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).wrapping_mul(2_246_822_519);
        image::Rgb([(v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8])
    });
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

#[test]
fn the_pick_is_the_pixel_the_canvas_shows_at_its_density_preview_and_detail() {
    let mut s = Session::new();
    // Anti-aliased edges, a photo-like image and small layers drawn approximately when zoomed out.
    rect(&mut s, (3.3, 2.7, 41.1, 37.9), "#2266DD");
    let data = base64::engine::general_purpose::STANDARD.encode(noise_png(97, 89));
    s.execute("file.placeImage", &json!({"data": data, "x": 60.4, "y": 3.1, "width": 77.3})).unwrap();
    for i in 0..40 {
        let fill = if i % 3 == 0 { "#DD3311" } else { "#11AA44" };
        rect(&mut s, (150.0 + f64::from(i % 8) * 1.5, 10.0 + f64::from(i / 8) * 1.5, 1.0, 1.0), fill);
    }
    let points = [(3.4, 20.0), (44.35, 21.0), (23.0, 2.8), (61.13, 7.77), (99.99, 40.04), (120.6, 50.2), (152.3, 12.1), (155.7, 14.9)];
    let views: [(f64, bool, u8, &[f64]); 4] =
        [(1.0, false, 1, &[0.1, 0.37, 1.0, 1.7]), (2.0, false, 1, &[0.2, 0.37, 1.0, 2.5]), (1.0, true, 1, &[4.0, 8.0]), (2.0, true, 2, &[6.0])];
    for (ppp, preview, k, zooms) in views {
        {
            let v = &mut s.ds_mut().unwrap().view;
            v.pixels_per_point = ppp;
            v.pixel_preview = preview;
            v.pixel_scale = k;
        }
        for &zoom in zooms {
            for (x, y) in points {
                assert_eq!(rgb(pick(&mut s, x, y, zoom)), shown(&s, x, y, zoom), "({x}, {y}) at zoom {zoom}, density {ppp}, pixel preview {preview}");
            }
        }
    }
}

#[test]
fn a_solid_fill_is_picked_as_stored() {
    // A colour between 8-bit steps (an HSB edit, a converted file) comes back as it is stored,
    // in either colour space, at any zoom; so does the page's colour.
    for profile in ["srgb", "displayP3"] {
        let mut s = Session::new();
        s.execute("file.colorProfile", &json!({"profile": profile})).unwrap();
        let stored = Color { r: 0.5, g: 0.123_45, b: 0.9, a: 1.0 };
        let r = rect(&mut s, (0.37, 0.61, 50.3, 40.7), "#000000");
        s.doc_mut().unwrap().get_mut(r).unwrap().fills = vec![Paint::solid(stored)];
        let bg = Color { r: 0.2, g: 0.333_33, b: 0.71, a: 1.0 };
        let page = s.page().unwrap();
        if let figcraft_doc::NodeKind::Page { background, .. } = &mut s.doc_mut().unwrap().get_mut(page).unwrap().kind {
            *background = bg;
        }
        for zoom in [0.3, 1.0, 2.7, 40.0] {
            assert_eq!(pick(&mut s, 20.13, 20.77, zoom), stored, "{profile}, zoom {zoom}");
            assert_eq!(pick(&mut s, 80.0, 80.0, zoom), bg, "{profile}, zoom {zoom}: the page");
        }
        // Every 8-bit colour picks back as itself.
        for v in (0..=255u8).step_by(5) {
            let c = Color::from_rgba8(v, 255 - v, v / 3, 255);
            s.execute("node.set", &json!({"id": r, "props": {"fill": c.to_css_hex()}})).unwrap();
            assert_eq!(pick(&mut s, 25.0, 25.0, 1.3).to_css_hex(), c.to_css_hex(), "{profile}");
        }
    }
}

#[test]
fn junk_pointer_positions_and_zooms_pick_without_crashing() {
    let mut s = Session::new();
    rect(&mut s, (0.0, 0.0, 10.0, 10.0), "#123456");
    for density in [1.0, f64::NAN, 0.0, -2.0, f64::INFINITY, 1e300] {
        s.ds_mut().unwrap().view.pixels_per_point = density;
        for (x, y, zoom) in [
            (5.0, 5.0, f64::NAN),
            (f64::NAN, 5.0, 1.0),
            (f64::INFINITY, 0.0, 1.0),
            (1e300, -1e300, 256.0),
            (5.0, 5.0, 0.0),
            (5.0, 5.0, -3.0),
            (5.0, 5.0, 1e300),
        ] {
            s.execute("edit.pickColor", &json!({})).unwrap();
            s.pointer(&PointerEvent::at(PointerKind::Down, x, y, zoom)).unwrap();
        }
    }
}
