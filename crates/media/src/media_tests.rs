//! `Media`: opening, pictures by time, sniffing, the shared cache and the refusals' wording, over
//! the synthetic source and the test writers.

use std::sync::{Arc, Mutex, PoisonError};

use crate::testing::{self, frame_index};
use crate::{Container, Media, MediaError, MediaKind, shared, sniff};

/// The shared cache is one per process: the tests that look at it take turns.
static CACHE: Mutex<()> = Mutex::new(());

fn open(bytes: Vec<u8>) -> Result<Media, MediaError> {
    Media::open(Arc::new(bytes))
}

fn index_at(m: &Media, t: f64) -> Option<u32> {
    frame_index(&m.frame_at(t).unwrap())
}

#[test]
fn the_synthetic_source_opens_as_a_video_whose_pictures_show_their_index() {
    let m = open(testing::synthetic(64, 36, 30, 10)).unwrap();
    let i = m.info();
    assert_eq!((i.container, i.kind, i.width, i.height, i.frame_count), (Container::Synthetic, MediaKind::Video, 64, 36, 30));
    assert!((i.duration - 3.0).abs() < 1e-9 && (i.frame_rate - 10.0).abs() < 1e-9 && !i.has_audio);
    // (time, picture): a picture shows from its start until the next one's; before 0 and not a
    // number the first; at and after the end the last.
    let cases = [
        (0.0, 0),
        (0.05, 0),
        (0.1, 1),
        (0.1999, 1),
        (0.25, 2),
        (2.95, 29),
        (3.0, 29),
        (99.0, 29),
        (-1.0, 0),
        (f64::NAN, 0),
        (f64::INFINITY, 29),
        (f64::NEG_INFINITY, 0),
        (f64::MAX, 29),
    ];
    for (t, want) in cases {
        assert_eq!(index_at(&m, t), Some(want), "t = {t}");
        assert_eq!(m.frame_index_at(t), want, "t = {t}");
    }
    assert!((m.frame_start(3) - 0.3).abs() < 1e-9);
    assert!((m.frame_start(1000) - 2.9).abs() < 1e-9, "past the end: the last picture's start");
    // A picture past the end is the last.
    assert_eq!(frame_index(&m.frame(1000).unwrap()), Some(29));
    // Pictures are made once and kept.
    let (a, b) = (m.frame(7).unwrap(), m.frame(7).unwrap());
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!((a.width, a.height, a.rgba.len()), (64, 36, 64 * 36 * 4));
    assert_eq!(a.pixel(0, 0), Some([7, 0, 0x80, 255]));
    assert_eq!(a.pixel(64, 0), None);
    assert_eq!(a.pixel(0, 36), None);
    assert_eq!(a.pixel(u32::MAX, u32::MAX), None);
    // Index 300 needs two bytes.
    let long = open(testing::synthetic(16, 16, 400, 25)).unwrap();
    assert_eq!(frame_index(&long.frame(300).unwrap()), Some(300));
}

#[test]
fn the_synthetic_source_can_have_sound_and_refuses_nonsense() {
    let with = open(testing::SyntheticSpec { width: 8, height: 8, frames: 2, fps: 2, audio: true }.bytes()).unwrap();
    assert!(with.info().has_audio);
    for (w, h, n, fps) in [(0, 8, 1, 1), (8, 0, 1, 1), (8, 8, 0, 1), (8, 8, 1, 0), (20_000, 20_000, 1, 1), (8, 8, u32::MAX, 1)] {
        let e = open(testing::synthetic(w, h, n, fps)).unwrap_err();
        assert!(matches!(e, MediaError::Corrupt(_) | MediaError::TooLarge(_)), "{w}x{h} {n}@{fps}: {e:?}");
    }
    // Cut short.
    assert!(open(testing::synthetic(8, 8, 2, 2)[..12].to_vec()).is_err());
}

#[test]
fn sniffing_tells_the_layouts_apart() {
    let still_png = {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    };
    let colors = [([255, 0, 0, 255], 100), ([0, 255, 0, 255], 100)];
    let cases: Vec<(&str, Vec<u8>, Option<Container>)> = vec![
        ("gif", testing::gif(&colors, 4, 4), Some(Container::Gif)),
        ("apng", testing::apng(&colors, 4, 4), Some(Container::Apng)),
        ("webp animation", testing::webp_animation(&colors, 4, 4), Some(Container::WebP)),
        ("png", still_png, None),
        ("avi", testing::avi_mjpeg(2, 16, 16, 10, true, false), Some(Container::Avi)),
        ("mov", testing::mov_mjpeg(2, 16, 16, 10, false), Some(Container::Mov)),
        ("mp4", testing::mp4_mjpeg(2, 16, 16, 10), Some(Container::Mp4)),
        ("webm", testing::matroska("webm", "V_VP9"), Some(Container::Matroska)),
        ("synthetic", testing::synthetic(8, 8, 2, 2), Some(Container::Synthetic)),
        ("empty", vec![], None),
        ("text", b"hello, this is not a video at all".to_vec(), None),
        ("riff of another kind", b"RIFF\x04\0\0\0WAVE".to_vec(), None),
        ("short", b"RIFF".to_vec(), None),
        ("jpeg", crate::testing::jpeg_picture(8, 8, 0), None),
    ];
    for (name, bytes, want) in cases {
        assert_eq!(sniff(&bytes), want, "{name}");
        // Every prefix gives an answer.
        for n in 0..bytes.len().min(64) {
            let _ = sniff(&bytes[..n]);
        }
    }
    // A QuickTime file without `ftyp` is still QuickTime.
    let mut moov_first = vec![0, 0, 0, 8];
    moov_first.extend_from_slice(b"moov");
    assert_eq!(sniff(&moov_first), Some(Container::Mov));
    assert_eq!(sniff(b"\0\0\0\x14ftypisom\0\0\0\0isom"), Some(Container::Mp4));
}

#[test]
fn what_is_not_media_is_not_opened() {
    for bytes in [vec![], b"junk".to_vec(), vec![0xFF, 0xD8, 0xFF, 0xD9], testing::jpeg_picture(8, 8, 0)] {
        assert_eq!(open(bytes).unwrap_err(), MediaError::NotMedia);
    }
}

#[test]
fn refusals_name_the_codec_in_a_sentence() {
    let e = MediaError::UnsupportedCodec { container: Container::Mp4, codec: "H.264 (avc1)".into() };
    assert_eq!(e.to_string(), "This video codec isn't supported yet: H.264 (avc1)");
    for e in [
        MediaError::NotMedia,
        MediaError::Unsupported("This video is a fragmented MP4, which isn't supported yet".into()),
        MediaError::Corrupt("a picture is outside the file".into()),
        MediaError::TooLarge("This video is too long".into()),
    ] {
        let t = e.to_string();
        assert!(t.chars().next().is_some_and(char::is_uppercase) && !t.ends_with('.'), "{t}");
    }
    assert_eq!(Container::Avi.label(), "AVI");
}

#[test]
fn files_are_opened_once_and_failures_are_remembered() {
    let _turn = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    let bytes = Arc::new(testing::synthetic(8, 8, 4, 4));
    let (a, b) = (shared("media-tests-a", &bytes).unwrap(), shared("media-tests-a", &bytes).unwrap());
    assert!(Arc::ptr_eq(&a, &b));
    // A bad file: the same answer again, without parsing it again (the second ask is given other
    // bytes, which would open if they were looked at).
    let junk = Arc::new(b"not media".to_vec());
    assert_eq!(shared("media-tests-b", &junk).unwrap_err(), MediaError::NotMedia);
    assert_eq!(shared("media-tests-b", &bytes).unwrap_err(), MediaError::NotMedia);
    crate::forget_all();
    assert!(shared("media-tests-b", &bytes).is_ok());
}

#[test]
fn many_files_do_not_all_stay_open() {
    let _turn = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    for i in 0..40 {
        let bytes = Arc::new(testing::synthetic(8, 8, 2 + i, 4));
        assert!(shared(&format!("media-tests-many-{i}"), &bytes).is_ok());
    }
    // The newest is still there (asked for with other bytes: opened again, it would have 7 pictures).
    let other = Arc::new(testing::synthetic(8, 8, 7, 4));
    assert_eq!(shared("media-tests-many-39", &other).unwrap().info().frame_count, 41, "the newest stays");
    // The oldest went.
    assert_eq!(shared("media-tests-many-0", &other).unwrap().info().frame_count, 7, "the oldest was let go");
}
