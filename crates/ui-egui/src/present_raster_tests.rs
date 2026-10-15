//! A tiny software renderer for egui frames, so the presenter's chrome can be looked at without a
//! window or a GPU: [`Rec`] drives the app headless like `present_tests::Ui` does and keeps every
//! texture egui sends (the font atlas, the presented frames), [`Rec::save`] draws the last frame's
//! meshes into a PNG. Premultiplied sRGB blending, as egui paints in gamma space; bilinear texture
//! sampling; no anti-aliasing beyond egui's own feathering.
//!
//! `snapshots` (below) is the presenter's gallery: it writes one PNG per control into the directory
//! named by `FIGCRAFT_SNAPSHOTS` and does nothing when that is unset.

use std::collections::HashMap;

use egui::epaint::{ClippedPrimitive, Primitive};
use egui::{Color32, ColorImage, Event, Pos2, Rect, TextureId, vec2};
use figcraft_engine::Session;
use serde_json::Value;

use super::ux;
use crate::present_tests::Ui;

/// The textures egui has sent so far.
#[derive(Default)]
pub(crate) struct Raster {
    textures: HashMap<TextureId, ColorImage>,
}

impl Raster {
    /// Apply the textures a frame sent (whole images and patches).
    pub fn feed(&mut self, delta: &egui::TexturesDelta) {
        for (id, deltas) in &delta.set {
            for d in deltas {
                let egui::ImageData::Color(img) = &d.image;
                match d.pos {
                    None => {
                        self.textures.insert(*id, ColorImage::clone(img));
                    }
                    Some([x0, y0]) => {
                        let Some(dst) = self.textures.get_mut(id) else { continue };
                        for y in 0..img.size[1] {
                            for x in 0..img.size[0] {
                                if x0 + x < dst.size[0] && y0 + y < dst.size[1] {
                                    dst.pixels[(y0 + y) * dst.size[0] + x0 + x] = img.pixels[y * img.size[0] + x];
                                }
                            }
                        }
                    }
                }
            }
        }
        for id in &delta.free {
            self.textures.remove(id);
        }
    }

    fn sample(&self, id: TextureId, uv: Pos2) -> [f32; 4] {
        let Some(t) = self.textures.get(&id) else { return [1.0; 4] };
        let (w, h) = (t.size[0] as f32, t.size[1] as f32);
        let (fx, fy) = (uv.x * w - 0.5, uv.y * h - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let at = |x: f32, y: f32| {
            let (x, y) = ((x.max(0.0) as usize).min(t.size[0] - 1), (y.max(0.0) as usize).min(t.size[1] - 1));
            let c = t.pixels[y * t.size[0] + x];
            [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0, f32::from(c.a()) / 255.0]
        };
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
        let mut out = [0.0; 4];
        for k in 0..4 {
            out[k] = (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty;
        }
        out
    }

    /// The frame's primitives drawn at `ppp` pixels per point into a `w`×`h` image over `bg`.
    pub fn draw(&self, prims: &[ClippedPrimitive], ppp: f32, (w, h): (usize, usize), bg: Color32) -> image::RgbaImage {
        let mut buf = vec![[f32::from(bg.r()) / 255.0, f32::from(bg.g()) / 255.0, f32::from(bg.b()) / 255.0, 1.0]; w * h];
        for p in prims {
            let Primitive::Mesh(m) = &p.primitive else { continue };
            let clip = Rect::from_min_max(p.clip_rect.min * ppp, p.clip_rect.max * ppp);
            let (cx0, cy0) = ((clip.min.x.floor().max(0.0)) as usize, (clip.min.y.floor().max(0.0)) as usize);
            let (cx1, cy1) = ((clip.max.x.ceil().max(0.0) as usize).min(w), (clip.max.y.ceil().max(0.0) as usize).min(h));
            for tri in m.indices.chunks_exact(3) {
                let v = |i: u32| m.vertices.get(i as usize).copied();
                let (Some(a), Some(b), Some(c)) = (v(tri[0]), v(tri[1]), v(tri[2])) else { continue };
                let (pa, pb, pc) = (a.pos * ppp, b.pos * ppp, c.pos * ppp);
                let area = (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x);
                if area.abs() < 1e-6 {
                    continue;
                }
                let x0 = (pa.x.min(pb.x).min(pc.x).floor().max(cx0 as f32)) as usize;
                let y0 = (pa.y.min(pb.y).min(pc.y).floor().max(cy0 as f32)) as usize;
                let x1 = (pa.x.max(pb.x).max(pc.x).ceil().min(cx1 as f32)) as usize;
                let y1 = (pa.y.max(pb.y).max(pc.y).ceil().min(cy1 as f32)) as usize;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let q = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                        let w0 = ((pb.x - q.x) * (pc.y - q.y) - (pb.y - q.y) * (pc.x - q.x)) / area;
                        let w1 = ((pc.x - q.x) * (pa.y - q.y) - (pc.y - q.y) * (pa.x - q.x)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                            continue;
                        }
                        let uv = Pos2::new(w0 * a.uv.x + w1 * b.uv.x + w2 * c.uv.x, w0 * a.uv.y + w1 * b.uv.y + w2 * c.uv.y);
                        let col =
                            |f: fn(Color32) -> u8| (w0 * f32::from(f(a.color)) + w1 * f32::from(f(b.color)) + w2 * f32::from(f(c.color))) / 255.0;
                        let (vr, vg, vb, va) = (col(|c| c.r()), col(|c| c.g()), col(|c| c.b()), col(|c| c.a()));
                        let t = self.sample(m.texture_id, uv);
                        let (sr, sg, sb, sa) = (vr * t[0], vg * t[1], vb * t[2], va * t[3]);
                        let dst = &mut buf[y * w + x];
                        *dst = [sr + dst[0] * (1.0 - sa), sg + dst[1] * (1.0 - sa), sb + dst[2] * (1.0 - sa), sa + dst[3] * (1.0 - sa)];
                    }
                }
            }
        }
        let mut img = image::RgbaImage::new(w as u32, h as u32);
        for (i, px) in buf.iter().enumerate() {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            img.put_pixel((i % w) as u32, (i / w) as u32, image::Rgba([q(px[0]), q(px[1]), q(px[2]), 255]));
        }
        img
    }
}

/// The app driven headless that remembers what it drew.
pub(crate) struct Rec {
    pub u: Ui,
    raster: Raster,
    last: Vec<egui::epaint::ClippedShape>,
}

impl Rec {
    pub fn new(s: Session) -> Self {
        let u = Ui {
            ctx: egui::Context::default(),
            app: crate::FigApp::new(s, Box::new(crate::NoServices)),
            time: 0.0,
            size: vec2(1440.0, 900.0),
            repaint: None,
        };
        let mut r = Self { u, raster: Raster::default(), last: Vec::new() };
        for _ in 0..3 {
            r.frame(Vec::new());
        }
        r
    }

    /// A smaller or larger window (points).
    #[allow(dead_code)]
    pub fn resize(&mut self, w: f32, h: f32) {
        self.u.size = vec2(w, h);
        self.frame(Vec::new());
    }

    pub fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
        self.u.time += 1.0 / 60.0;
        let input =
            egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.u.size)), time: Some(self.u.time), events, ..Default::default() };
        let app = &mut self.u.app;
        let mut out = self.u.ctx.run_ui(input, |ui| app.ui(ui));
        self.raster.feed(&out.textures_delta);
        out.textures_delta.clear();
        self.last = out.shapes.clone();
        out
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(Vec::new());
        }
    }

    /// Run a command like `Ui::run` does, then a frame.
    pub fn run(&mut self, cmd: &str, params: Value) -> Value {
        let r = self.u.app.session.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd} {params}: {e}"));
        self.u.app.handle_ui_requests();
        self.frame(Vec::new());
        r
    }

    /// The last frame as a PNG at `path` (the window's size, 1 pixel per point).
    pub fn save(&self, path: &std::path::Path) {
        self.save_region(path, Rect::from_min_size(Pos2::ZERO, self.u.size), 1);
    }

    /// `region` of the last frame (points), each point drawn as `zoom`×`zoom` pixels.
    pub fn save_region(&self, path: &std::path::Path, region: Rect, zoom: u32) {
        let z = zoom.max(1) as f32;
        let prims = self.u.ctx.tessellate(self.last.clone(), z);
        let full = self.raster.draw(&prims, z, ((self.u.size.x * z) as usize, (self.u.size.y * z) as usize), Color32::from_gray(40));
        let (x, y) = ((region.min.x.max(0.0) * z) as u32, (region.min.y.max(0.0) * z) as u32);
        let (w, h) =
            (((region.width() * z) as u32).min(full.width().saturating_sub(x)), ((region.height() * z) as u32).min(full.height().saturating_sub(y)));
        let img = image::imageops::crop_imm(&full, x, y, w.max(1), h.max(1)).to_image();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        img.save(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

#[test]
fn the_rasterizer_draws_the_presenter() {
    let mut s = Session::new();
    let a = crate::present_tests::id(
        &s.execute("frame.create", &serde_json::json!({"x": 0, "y": 0, "width": 300, "height": 400, "name": "Home"})).unwrap(),
    );
    let mut r = Rec::new(s);
    r.run("proto.present", serde_json::json!({"start": a}));
    let prims = r.u.ctx.tessellate(r.last.clone(), 1.0);
    let img = r.raster.draw(&prims, 1.0, (1440, 900), Color32::BLACK);
    // The bar is dark grey at the top, the page colour fills the rest, and the bar's text is drawn
    // from the font atlas (some pixels there are lighter than the bar).
    assert_eq!(img.get_pixel(700, 10).0[..3], [0x2C, 0x2C, 0x2C]);
    assert_ne!(img.get_pixel(30, 450).0[..3], [0x2C, 0x2C, 0x2C]);
    let lit = (0..1440).flat_map(|x| (0..40).map(move |y| (x, y))).filter(|&(x, y)| img.get_pixel(x, y).0[0] > 0xA0).count();
    assert!(lit > 50, "text drawn in the bar: {lit} light pixels");
}

/// Where `snapshots` writes (None: it does nothing).
fn gallery_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("FIGCRAFT_SNAPSHOTS").map(Into::into)
}

/// Three screens of a small app: a coloured card, a title and a button that opens the next one.
fn demo() -> (Rec, [figcraft_doc::NodeId; 3]) {
    use serde_json::json;
    let mut s = Session::new();
    let id = |v: Value| crate::present_tests::id(&v);
    let mut frames = Vec::new();
    for (i, name) in ["Welcome", "Sign in", "Home"].into_iter().enumerate() {
        let colour = ["#0D99FF", "#9747FF", "#14AE5C"][i];
        let f = id(s.execute("frame.create", &json!({"x": 420.0 * i as f64, "y": 0, "width": 390, "height": 760, "name": name})).unwrap());
        s.execute("fill.set", &json!({"ids": [f], "color": "#FFFFFF"})).unwrap();
        let hero = id(s
            .execute(
                "shape.rectangle",
                &json!({"parent": f, "x": 24, "y": 80, "width": 342, "height": 220, "props": {"name": "Hero", "fill": colour}}),
            )
            .unwrap());
        s.execute("node.set", &json!({"id": hero, "props": {"cornerRadius": 16}})).unwrap();
        s.execute("text.create", &json!({"parent": f, "text": name, "x": 24, "y": 330})).unwrap();
        let button = id(s
            .execute(
                "shape.rectangle",
                &json!({"parent": f, "x": 24, "y": 640, "width": 342, "height": 56, "props": {"name": "Button", "fill": "#1E1E1E"}}),
            )
            .unwrap());
        s.execute("node.set", &json!({"id": button, "props": {"cornerRadius": 12}})).unwrap();
        frames.push((f, button));
    }
    for w in frames.windows(2) {
        s.execute(
            "proto.interaction.add",
            &json!({"id": w[0].1, "trigger": {"type": "onClick"}, "action": {"type": "navigate", "dest": w[1].0}, "transition": {"type": "instant"}}),
        )
        .unwrap();
    }
    s.execute("proto.flow.add", &json!({"start": frames[0].0, "name": "Onboarding"})).unwrap();
    (Rec::new(s), [frames[0].0, frames[1].0, frames[2].0])
}

/// The presenter's gallery: `FIGCRAFT_SNAPSHOTS=/some/dir cargo test -p figcraft-ui-egui --lib snapshots`.
#[test]
fn snapshots() {
    let Some(dir) = gallery_dir() else { return };
    let (mut r, [a, ..]) = demo();
    r.run("proto.present", serde_json::json!({"start": a}));
    r.frames(2);
    r.save(&dir.join("presenter.png"));
    if let Some(p) = r.u.app.ui.present.as_mut() {
        p.ux.help = true;
    }
    // (A window fades in.)
    r.frames(40);
    r.save(&dir.join("shortcuts.png"));
    if let Some(p) = r.u.app.ui.present.as_mut() {
        p.ux.help = false;
    }
    // The second screen: the bar at 3×.
    r.u.app.run("select.set", serde_json::json!({"ids": []}));
    r.u.app.ui.present.as_mut().map(|p| ux::step(p, true, 1.0));
    r.frames(3);
    r.save_region(&dir.join("bar-counter.png"), Rect::from_min_size(Pos2::ZERO, vec2(480.0, 40.0)), 3);
    // The preview pane over the editor.
    r.u.app.ui.present = None;
    r.run("proto.preview", serde_json::json!({"start": a}));
    r.frames(40);
    r.save(&dir.join("pane.png"));
}
