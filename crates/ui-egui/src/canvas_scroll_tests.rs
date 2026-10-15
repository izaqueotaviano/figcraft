//! Scrolling the canvas shows rendered pixels, not a strip of bare page background, and pans
//! within the overscan render (and upload) nothing.

use std::time::Instant;

use figcraft_render::{Retained, View};

use crate::canvas::plan_view;

/// The screen view after `i` frames of scrolling down `step` device pixels a frame.
fn screen(i: usize, step: f64) -> View {
    View { origin: (0.0, i as f64 * step), scale: 1.0, width: 1920, height: 1080 }
}

fn covers(have: &View, want: &View) -> bool {
    let s = want.scale;
    let (l, t) = ((want.origin.0 - have.origin.0) * s, (want.origin.1 - have.origin.1) * s);
    (have.scale - s).abs() < 1e-9
        && l >= 0.0
        && t >= 0.0
        && l + f64::from(want.width) <= f64::from(have.width)
        && t + f64::from(want.height) <= f64::from(have.height)
}

/// Frames that show uncovered background, and renders asked for, scrolling `frames` frames when a
/// render takes `latency` frames to arrive. `plan` picks each frame's render from the last one.
fn scroll(frames: usize, latency: usize, plan: impl Fn(&View, Option<&View>) -> View) -> (usize, usize, Vec<View>) {
    let mut asked: Vec<(usize, View)> = vec![];
    let mut bare = 0;
    for i in 0..frames {
        let want = screen(i, 24.0);
        let v = plan(&want, asked.last().map(|(_, v)| v));
        if asked.last().map(|(_, l)| l) != Some(&v) {
            asked.push((i, v));
        }
        // The newest render that has arrived by now.
        let shown = asked.iter().rev().find(|(at, _)| at + latency <= i).map(|(_, v)| v);
        if !shown.is_some_and(|s| covers(s, &want)) && i >= latency {
            bare += 1;
        }
    }
    let n = asked.len();
    (bare, n, asked.into_iter().map(|(_, v)| v).collect())
}

#[test]
fn scrolling_shows_rendered_pixels_and_renders_only_at_the_overscan_edge() {
    // Before: the exact viewport, every frame it moved.
    let (bare_old, renders_old, _) = scroll(300, 3, |w, _| w.clone());
    // Now: overscan, reused while it covers the screen.
    let (bare_new, renders_new, _) = scroll(300, 3, |w, h| plan_view(w, h, 16_384));
    assert_eq!(renders_old, 300);
    assert!(bare_old > 290, "{bare_old}");
    assert_eq!(bare_new, 0, "a strip of background showed while scrolling");
    // One render each time the pan has used half the margin (every ~6 frames here), not every frame.
    assert!(renders_new * 5 < renders_old, "{renders_new} renders for 300 frames");
}

#[test]
fn a_still_view_keeps_its_render_and_a_zoom_asks_for_a_new_one() {
    let w = screen(0, 0.0);
    let first = plan_view(&w, None, 16_384);
    assert!(covers(&first, &w));
    assert_eq!(plan_view(&w, Some(&first), 16_384), first);
    let zoomed = View { scale: 2.0, ..w.clone() };
    let z = plan_view(&zoomed, Some(&first), 16_384);
    assert!(covers(&z, &zoomed) && z.scale == 2.0);
    // Junk views don't panic and stay within the texture limit.
    let junk = View { origin: (f64::NAN, f64::INFINITY), scale: 0.0, width: u32::MAX, height: 0 };
    let j = plan_view(&junk, Some(&first), 16_384);
    assert!(j.width <= 16_384 && j.height <= 16_384);
    // The overscan stays within the GPU's texture limit, with the pixel the renderer adds to snap.
    let big = View { width: 7680, height: 4320, ..w.clone() };
    let b = plan_view(&big, None, crate::canvas::render_cap(8192));
    assert!(b.width < 8192 && b.height < 8192 && covers(&b, &big), "{b:?}");
    let doc = std::sync::Arc::new(figcraft_doc::Document::new("x"));
    let mut r = Retained::default();
    let (_, pix, _) = r.render(&doc, doc.first_page().unwrap(), &View { width: 2047, ..w.clone() }, &Default::default()).unwrap();
    assert!(pix.width() <= 2048, "a render {} wide doesn't fit a 2048 texture", pix.width());
    assert_eq!(crate::canvas::render_cap(2048), 2047);
    // A screen bigger than a texture can be gets what fits rather than a texture the GPU refuses.
    let huge = plan_view(&View { width: 9000, height: 5000, ..w.clone() }, None, crate::canvas::render_cap(8192));
    assert!(huge.width < 8192 && huge.height < 8192, "{huge:?}");
}

/// Render time scrolling and zooming a large document, old policy vs new
/// (`cargo test -p figcraft-ui-egui --lib bench_scroll -- --ignored --nocapture`).
#[test]
#[ignore = "benchmark"]
fn bench_scroll_render_time() {
    let (doc, _) = figcraft_engine::synthetic::large_document(20_000).unwrap();
    let page = doc.first_page().unwrap();
    let doc = std::sync::Arc::new(doc);
    let opts = figcraft_render::Options { lod: true, ..Default::default() };
    // What the render thread does for each job (render, then the texture image), and the bytes
    // the UI thread then uploads.
    let time = |views: &[View]| {
        let mut r = Retained::default();
        let (mut total, mut worst) = (0.0f64, 0.0f64);
        let mut upload = 0u64;
        for v in views {
            let t = Instant::now();
            if let Some((_, pix, _)) = r.render(&doc, page, v, &opts) {
                let image = crate::render_worker::screen_image(pix, doc.color_profile);
                upload += image.pixels.len() as u64 * 4;
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            total += ms;
            worst = worst.max(ms);
        }
        (total, worst, upload as f64 / 1e6)
    };
    // Scrolling down 24 device pixels a frame for 300 frames.
    let exact = |w: &View, _: Option<&View>| w.clone();
    let planned = |w: &View, h: Option<&View>| plan_view(w, h, 16_384);
    for (name, views) in [("scroll, exact viewport each frame", scroll(300, 3, exact).2), ("scroll, overscan + reuse", scroll(300, 3, planned).2)] {
        let (ms, worst, mb) = time(&views);
        println!("{name}: {} renders, {ms:.0} ms on the render thread (slowest {worst:.1} ms), {mb:.0} MB of texture uploads", views.len());
    }
    // Zooming in 5% a frame for 30 frames about the centre, then settling.
    let zoom_view = |i: usize, draft: bool| {
        let z = 1.05f64.powi(i as i32);
        let k = if draft { 0.5 } else { 1.0 };
        let (w, h) = (1920.0, 1080.0);
        View { origin: (w / 2.0 - w / 2.0 / z, h / 2.0 - h / 2.0 / z), scale: z * k, width: (w * k) as u32, height: (h * k) as u32 }
    };
    let old: Vec<View> = (0..=30).map(|i| zoom_view(i, false)).collect();
    let mut new: Vec<View> = (0..30).map(|i| zoom_view(i, true)).collect();
    new.push(plan_view(&zoom_view(30, false), None, 16_384));
    for (name, views) in [("zoom, full resolution each frame", old), ("zoom, half-resolution drafts + final", new)] {
        let (ms, worst, mb) = time(&views);
        println!("{name}: {} renders, {ms:.0} ms on the render thread (slowest {worst:.1} ms), {mb:.0} MB of texture uploads", views.len());
    }
}

/// UI frame time panning the canvas with the wheel, as the canvas does it now (a view change
/// leaves the document's `revision` alone) and as it did (a bump every frame, which rebuilt the
/// panels' per-document caches: the CJK name scan, the Assets listing…)
/// (`cargo test -p figcraft-ui-egui --lib bench_pan_ui -- --ignored --nocapture`).
#[test]
#[ignore = "benchmark"]
fn bench_pan_ui_frames() {
    use egui::{Event, Pos2, RawInput, Rect, vec2};
    figcraft_text::scan_system_fonts();
    let docs = [
        ("10k layers, layers panel", figcraft_engine::synthetic::large_document(10_000).unwrap().0, crate::LeftTab::File),
        (
            "2k components × 2k instances, Assets panel",
            figcraft_engine::synthetic_ds::design_system_with_instances(
                &figcraft_engine::synthetic_ds::DsSpec { icons: 2000, ..figcraft_engine::synthetic_ds::DsSpec::FULL },
                2000,
            )
            .unwrap()
            .0,
            crate::LeftTab::Assets,
        ),
    ];
    for (name, doc, tab) in docs {
        for bump in [true, false] {
            let ctx = egui::Context::default();
            let mut app = crate::FigApp::new(figcraft_engine::Session::with_document(doc.clone()), Box::new(crate::NoServices));
            app.ui.home = false;
            app.ui.left_tab = tab;
            let mut time = 0.0;
            let mut frame = |app: &mut crate::FigApp, events: Vec<Event>| {
                time += 1.0 / 60.0;
                let input = RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                };
                let t = Instant::now();
                ctx.run_ui(input, |ui| app.ui(ui)).textures_delta.clear();
                t.elapsed().as_secs_f64() * 1000.0
            };
            for _ in 0..30 {
                frame(&mut app, vec![]);
            }
            let centre = app.canvas.rect.center();
            let wheel = |d: f32| Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, d),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            };
            let mut total = 0.0;
            for i in 0..120 {
                if bump && let Ok(ds) = app.session.ds_mut() {
                    ds.revision += 1;
                }
                total += frame(&mut app, vec![Event::PointerMoved(centre), wheel(if i % 40 < 20 { -40.0 } else { 40.0 })]);
            }
            let how = if bump { "before (revision bumped each frame)" } else { "now" };
            println!("pan frames, {name}, {how}: {:.2} ms a frame", total / 120.0);
        }
    }
}
