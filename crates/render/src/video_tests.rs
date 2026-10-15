//! Video fills draw the picture at their poster time (the editor, exports and screens that don't
//! play them), laid out like image fills; missing and broken videos draw nothing.

use std::sync::Arc;

use figcraft_color::{ColorProfile, ImageScaleMode, Paint, PaintKind};
use figcraft_doc::{Document, ImageData, NodeId, NodeKind};
use figcraft_media::testing;

use crate::images;

/// A page with a rectangle of `w` × `h` filled with the synthetic video `bytes` (kept under
/// `hash`: tests use their own, since opened files are shared by hash).
fn scene(hash: &str, bytes: Vec<u8>, w: f64, h: f64, mode: ImageScaleMode, poster: f64) -> (Document, NodeId) {
    let mut d = Document::new("t");
    let page = d.add_page("Page 1");
    let id = d.create(page, NodeKind::Rectangle, "Video", 0.0, 0.0, w, h).unwrap();
    d.images.insert(hash.to_string(), ImageData { bytes: Arc::new(bytes), width: 64, height: 48 });
    let mut paint = Paint::video(hash, mode);
    if let PaintKind::Video(v) = &mut paint.kind {
        v.poster = poster;
    }
    d.get_mut(id).unwrap().fills = vec![paint];
    (d, id)
}

fn px(p: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 4] {
    let c = p.pixel(x, y).unwrap();
    [c.red(), c.green(), c.blue(), c.alpha()]
}

/// The picture index a rendered synthetic video shows at (x, y) inside its top-left marker.
fn shown(p: &tiny_skia::Pixmap, x: u32, y: u32) -> Option<u32> {
    let [r, g, b, a] = px(p, x, y);
    // (The marker is blue 0x80, red/green carry the index; bicubic filtering may be a step off.)
    (a == 255 && b.abs_diff(0x80) <= 2).then(|| u32::from(r) | u32::from(g) << 8)
}

#[test]
fn a_video_layer_shows_its_poster_picture() {
    // 30 pictures at 10 per second: three seconds.
    let bytes = testing::synthetic(64, 48, 30, 10);
    for (poster, want) in [(0.0, 0), (1.0, 10), (1.05, 10), (2.95, 29), (3.0, 29), (500.0, 29)] {
        let (d, id) = scene("video-poster-a", bytes.clone(), 64.0, 48.0, ImageScaleMode::Fill, poster);
        let p = crate::render_node(&d, id, 1.0).unwrap();
        assert_eq!(shown(&p, 3, 3), Some(want), "poster {poster}");
    }
    // Junk poster times are the first picture.
    for poster in [-5.0, f64::NAN, f64::NEG_INFINITY] {
        let (d, id) = scene("video-poster-a", bytes.clone(), 64.0, 48.0, ImageScaleMode::Fill, poster);
        let p = crate::render_node(&d, id, 1.0).unwrap();
        assert_eq!(shown(&p, 3, 3), Some(0), "poster {poster}");
    }
}

#[test]
fn fill_covers_the_layer_and_fit_keeps_the_picture_whole() {
    let bytes = testing::synthetic(64, 48, 30, 10);
    // A layer twice as wide: Fit puts the 64 × 48 picture in the middle at its own size.
    let (d, id) = scene("video-poster-b", bytes.clone(), 128.0, 48.0, ImageScaleMode::Fit, 0.5);
    let p = crate::render_node(&d, id, 1.0).unwrap();
    assert_eq!(px(&p, 3, 3)[3], 0, "left of the picture is empty");
    assert_eq!(shown(&p, 32 + 3, 3), Some(5), "the picture starts 32 pixels in");
    assert_eq!(px(&p, 124, 40)[3], 0, "right of the picture is empty");
    // Fill on the wide layer covers all of it (the picture is scaled up and cropped top and bottom).
    let (d, id) = scene("video-poster-b", bytes.clone(), 128.0, 48.0, ImageScaleMode::Fill, 0.5);
    let p = crate::render_node(&d, id, 1.0).unwrap();
    assert!([(3, 3), (124, 3), (3, 44), (124, 44), (64, 24)].iter().all(|(x, y)| px(&p, *x, *y)[3] == 255), "Fill covers the whole layer");
    // Twice the size in both ways: the picture, twice as big, with its marker 16 pixels wide.
    let (d, id) = scene("video-poster-b", bytes, 128.0, 96.0, ImageScaleMode::Fill, 0.5);
    let p = crate::render_node(&d, id, 1.0).unwrap();
    assert_eq!(shown(&p, 6, 6), Some(5));
    assert_eq!(shown(&p, 12, 12), Some(5));
    assert_eq!(shown(&p, 20, 6), None, "past the marker");
}

#[test]
fn a_video_that_is_missing_or_broken_draws_nothing() {
    // Not in the document.
    let mut d = Document::new("t");
    let page = d.add_page("Page 1");
    let id = d.create(page, NodeKind::Rectangle, "Video", 0.0, 0.0, 16.0, 16.0).unwrap();
    d.get_mut(id).unwrap().fills = vec![Paint::video("video-not-there", ImageScaleMode::Fill)];
    let p = crate::render_node(&d, id, 1.0).unwrap();
    assert!((0..16).all(|y| (0..16).all(|x| px(&p, x, y)[3] == 0)));
    // In the document, but not a file that opens.
    d.images.insert("video-broken".into(), ImageData { bytes: Arc::new(b"RIFF\x04\0\0\0AVI ".to_vec()), width: 0, height: 0 });
    d.get_mut(id).unwrap().fills = vec![Paint::video("video-broken", ImageScaleMode::Fill)];
    let p = crate::render_node(&d, id, 1.0).unwrap();
    assert!((0..16).all(|y| (0..16).all(|x| px(&p, x, y)[3] == 0)));
    assert!(images::video_frame("video-broken", d.images.get("video-broken"), 0.0, ColorProfile::Srgb).is_none());
}

#[test]
fn pictures_are_kept_for_the_next_ask() {
    let data = ImageData { bytes: Arc::new(testing::synthetic(32, 24, 40, 10)), width: 32, height: 24 };
    let a = images::video_frame("video-cache-a", Some(&data), 1.0, ColorProfile::Srgb).unwrap();
    let b = images::video_frame("video-cache-a", Some(&data), 1.04, ColorProfile::Srgb).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "the same picture asked for twice is decoded once");
    let c = images::video_frame("video-cache-a", Some(&data), 2.0, ColorProfile::Srgb).unwrap();
    assert!(!Arc::ptr_eq(&a, &c));
    // Asking for many pictures never holds them all.
    for i in 0..40 {
        let f = images::video_frame("video-cache-a", Some(&data), f64::from(i) / 10.0, ColorProfile::Srgb).unwrap();
        assert_eq!((f.width(), f.height()), (32, 24));
    }
    // A video isn't an image, and no picture is made for a time that isn't a number.
    assert!(images::video_frame("video-cache-a", Some(&data), f64::NAN, ColorProfile::Srgb).is_some());
    assert!(images::video_frame("video-cache-a", None, 0.0, ColorProfile::Srgb).is_none());
}

#[test]
fn rendering_just_the_area_a_video_covers_matches_the_whole_render() {
    // The presenter patches a moving video's area of the screen's texture: that area rendered
    // alone must be the very pixels the whole render has there, whatever is around the video.
    use figcraft_color::{Color, Paint};
    use figcraft_doc::Effect;
    use figcraft_geom::Rect;
    let bytes = testing::synthetic(64, 48, 30, 10);
    let mut d = Document::new("t");
    let page = d.add_page("Page 1");
    let root = d.create(page, NodeKind::Frame, "Screen", 0.0, 0.0, 200.0, 150.0).unwrap();
    let back = d.create(root, NodeKind::Rectangle, "Back", 0.0, 0.0, 200.0, 150.0).unwrap();
    d.get_mut(back).unwrap().fills = vec![Paint::solid(Color::from_hex("335577").unwrap())];
    let clip = d.create(root, NodeKind::Rectangle, "Clip", 20.0, 20.0, 96.0, 72.0).unwrap();
    d.images.insert("video-area".into(), ImageData { bytes: Arc::new(bytes), width: 64, height: 48 });
    {
        let n = d.get_mut(clip).unwrap();
        n.fills = vec![Paint::video("video-area", ImageScaleMode::Fill)];
        n.corners.radii = [14.0; 4];
        n.effects.push(Effect::drop_shadow());
    }
    // A layer above the video, over its corner, half see-through.
    let above = d.create(root, NodeKind::Rectangle, "Above", 90.0, 60.0, 60.0, 50.0).unwrap();
    d.get_mut(above).unwrap().fills = vec![Paint::solid(Color::from_hex("FFCC0099").unwrap())];
    for (scale, poster) in [(1.0, 0.0), (2.0, 1.3), (1.5, 2.9)] {
        if let PaintKind::Video(v) = &mut d.get_mut(clip).unwrap().fills[0].kind {
            v.poster = poster;
        }
        let bounds = crate::export_bounds(&d, root).unwrap();
        let full = crate::render_node_at(&d, root, scale, 0.0).unwrap();
        // The video's area with its shadow, in texture pixels, a pixel more all round.
        let b = crate::raster_bounds(&d, clip).unwrap();
        let (x0, y0) = ((((b.x0 - bounds.x0) * scale).floor() - 1.0).max(0.0), (((b.y0 - bounds.y0) * scale).floor() - 1.0).max(0.0));
        let (x1, y1) = (
            (((b.x1 - bounds.x0) * scale).ceil() + 1.0).min(f64::from(full.width())),
            (((b.y1 - bounds.y0) * scale).ceil() + 1.0).min(f64::from(full.height())),
        );
        let region = Rect::new(bounds.x0 + x0 / scale, bounds.y0 + y0 / scale, bounds.x0 + x1 / scale, bounds.y0 + y1 / scale);
        let part = crate::render_area_at(&d, &[root], region, scale, 0.0).unwrap();
        assert_eq!((f64::from(part.width()), f64::from(part.height())), (x1 - x0, y1 - y0), "scale {scale}");
        let mut worst = 0u8;
        for y in 0..part.height() {
            for x in 0..part.width() {
                let (a, b) = (px(&part, x, y), px(&full, x0 as u32 + x, y0 as u32 + y));
                worst = a.iter().zip(b).map(|(p, q)| p.abs_diff(q)).fold(worst, u8::max);
            }
        }
        assert!(worst <= 1, "scale {scale}, poster {poster}: the patch differs from the whole render by {worst}");
    }
}
