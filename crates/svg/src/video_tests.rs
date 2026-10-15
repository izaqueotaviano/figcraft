//! A video fill exports as its poster picture (a PNG in a pattern, like an image fill).

use std::sync::Arc;

use super::*;
use figcraft_color::{ColorProfile, ImageScaleMode, Paint, PaintKind};
use figcraft_doc::{Document, ImageData, NodeKind};
use figcraft_media::testing;

fn doc_with_video(hash: &str, poster: f64) -> (Document, NodeId) {
    let mut d = Document::new("t");
    let p = d.first_page().unwrap();
    let f = d.create(p, NodeKind::Frame, "Screen", 0.0, 0.0, 100.0, 80.0).unwrap();
    let r = d.create(f, NodeKind::Rectangle, "Clip", 10.0, 10.0, 64.0, 48.0).unwrap();
    d.images.insert(hash.into(), ImageData { bytes: Arc::new(testing::synthetic(64, 48, 30, 10)), width: 64, height: 48 });
    let mut paint = Paint::video(hash, ImageScaleMode::Fill);
    if let PaintKind::Video(v) = &mut paint.kind {
        v.poster = poster;
    }
    d.get_mut(r).unwrap().fills = vec![paint];
    (d, f)
}

#[test]
fn the_poster_picture_is_exported_as_a_png() {
    for poster in [0.0, 1.0] {
        let (d, f) = doc_with_video("svg-video-a", poster);
        let svg = export_svg(&d, &[f], &SvgOptions::default()).unwrap();
        let start = svg.find("data:image/png;base64,").expect("a PNG in the SVG") + "data:image/png;base64,".len();
        let b64 = &svg[start..start + svg[start..].find('"').unwrap()];
        let png = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        // The picture of the poster time, as the renderer shows it.
        let pm = figcraft_render::images::video_frame("svg-video-a", d.images.get("svg-video-a"), poster, ColorProfile::Srgb).unwrap();
        assert_eq!(png, figcraft_render::encode_png(&pm).unwrap(), "poster {poster}");
        assert!(svg.contains("<pattern"), "{svg}");
    }
    // Different posters, different pictures.
    let a = export_svg(&doc_with_video("svg-video-b", 0.0).0, &[doc_with_video("svg-video-b", 0.0).1], &SvgOptions::default()).unwrap();
    let b = export_svg(&doc_with_video("svg-video-c", 2.0).0, &[doc_with_video("svg-video-c", 2.0).1], &SvgOptions::default()).unwrap();
    assert_ne!(a.split("base64,").nth(1), b.split("base64,").nth(1));
}

#[test]
fn a_missing_video_exports_no_picture() {
    let (mut d, f) = doc_with_video("svg-video-gone", 0.0);
    d.images.clear();
    let svg = export_svg(&d, &[f], &SvgOptions::default()).unwrap();
    assert!(!svg.contains("<image"), "{svg}");
}
