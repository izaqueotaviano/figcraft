//! Video paints: what they store, what old and hostile data reads as.

use crate::{ImageScaleMode, Paint, PaintKind, VideoPaint};

fn video(p: &Paint) -> &VideoPaint {
    match &p.kind {
        PaintKind::Video(v) => v,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_video_paint_starts_playing_once_and_unmuted() {
    let p = Paint::video("abc", ImageScaleMode::Fit);
    let v = video(&p);
    assert_eq!((v.video.as_str(), v.scale_mode, v.poster, v.autoplay, v.looping, v.muted), ("abc", ImageScaleMode::Fit, 0.0, true, false, false));
    assert_eq!(p.label(), "Video");
    assert_eq!((p.opacity, p.visible), (1.0, true));
}

#[test]
fn it_is_stored_compactly_and_reads_back() {
    let mut p = Paint::video("abc", ImageScaleMode::Fill);
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["type"], "video");
    assert_eq!(j["video"], "abc");
    assert!(j.get("poster").is_none(), "a poster at 0 isn't written: {j}");
    assert_eq!((j["autoplay"].clone(), j["loop"].clone(), j["muted"].clone()), (true.into(), false.into(), false.into()));
    if let PaintKind::Video(v) = &mut p.kind {
        v.poster = 2.5;
        v.looping = true;
        v.muted = true;
        v.autoplay = false;
    }
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!((j["poster"].clone(), j["loop"].clone()), (2.5.into(), true.into()));
    let back: Paint = serde_json::from_value(j).unwrap();
    assert_eq!(back, p);
    // Only the asset is required; everything else has its default.
    let min: Paint = serde_json::from_str(r#"{"type":"video","video":"h"}"#).unwrap();
    let v = video(&min);
    assert!(v.autoplay && !v.looping && !v.muted && v.poster == 0.0 && v.scale_mode == ImageScaleMode::Fill);
    assert_eq!((min.opacity, min.visible), (1.0, true));
    assert!(serde_json::from_str::<Paint>(r#"{"type":"video"}"#).is_err(), "without a video there is nothing to play");
}

#[test]
fn hostile_poster_times_read_as_usable_ones() {
    let at = |t: f64| VideoPaint { poster: t, ..VideoPaint::new("h") }.poster_time();
    assert_eq!(at(2.5), 2.5);
    assert_eq!(at(-3.0), 0.0);
    assert_eq!(at(f64::NAN), 0.0);
    assert_eq!(at(f64::INFINITY), 0.0);
    assert_eq!(at(f64::NEG_INFINITY), 0.0);
    assert_eq!(at(1e300), VideoPaint::MAX_POSTER);
}

#[test]
fn it_lays_out_like_a_fill_or_fit_image() {
    let mut v = VideoPaint::new("h");
    for (mode, want) in [
        (ImageScaleMode::Fill, ImageScaleMode::Fill),
        (ImageScaleMode::Fit, ImageScaleMode::Fit),
        (ImageScaleMode::Crop, ImageScaleMode::Fill),
        (ImageScaleMode::Tile, ImageScaleMode::Fill),
    ] {
        v.scale_mode = mode;
        let img = v.as_image();
        assert_eq!((img.image.as_str(), img.scale_mode, img.rotation, img.crop), ("h", want, 0, None));
        assert!(img.filters.is_identity());
    }
}
