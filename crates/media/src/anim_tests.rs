//! Animated GIF, WebP and PNG: pictures, timing, loops, stills, caps.

use std::sync::Arc;

use crate::testing;
use crate::{Container, MAX_ANIMATION_FRAMES, Media, MediaError, MediaKind};

fn open(bytes: Vec<u8>) -> Result<Media, MediaError> {
    Media::open(Arc::new(bytes))
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn colour(m: &Media, t: f64) -> [u8; 3] {
    let [r, g, b, _] = m.frame_at(t).unwrap().pixel(1, 1).unwrap();
    [r, g, b]
}

#[test]
fn animated_images_open_with_their_pictures_and_timing() {
    let pictures = [(RED, 100), (GREEN, 200), (BLUE, 300)];
    let files: [(Container, &str, Vec<u8>); 3] = [
        (Container::Gif, "GIF", testing::gif(&pictures, 6, 4)),
        (Container::Apng, "APNG", testing::apng(&pictures, 6, 4)),
        (Container::WebP, "WebP", testing::webp_animation(&pictures, 6, 4)),
    ];
    for (container, codec, bytes) in files {
        let m = open(bytes).unwrap_or_else(|e| panic!("{codec}: {e}"));
        let i = m.info();
        assert_eq!((i.container, i.kind, i.codec.as_str()), (container, MediaKind::Animation, codec));
        assert_eq!((i.width, i.height, i.frame_count, i.has_audio), (6, 4, 3, false), "{codec}");
        assert!((i.duration - 0.6).abs() < 1e-9, "{codec}: {}", i.duration);
        assert!((i.frame_rate - 5.0).abs() < 1e-9);
        // The picture at each time; after the end the last (looping is the player's business).
        for (t, want) in [(0.0, [255, 0, 0]), (0.05, [255, 0, 0]), (0.1, [0, 255, 0]), (0.29, [0, 255, 0]), (0.3, [0, 0, 255]), (5.0, [0, 0, 255])] {
            // (The WebP decoder composites in a way that can leave a channel one short.)
            let got = colour(&m, t);
            assert!(got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 2), "{codec} at {t}: {got:?} vs {want:?}");
        }
        assert!((m.frame_start(2) - 0.3).abs() < 1e-9);
        // Pictures are whole canvases, kept.
        assert!(Arc::ptr_eq(&m.frame(1).unwrap(), &m.frame(1).unwrap()));
    }
}

#[test]
fn quick_pictures_show_for_a_tenth_of_a_second_like_browsers_do() {
    for delay in [0, 10, 19] {
        let m = open(testing::gif(&[(RED, delay), (GREEN, delay)], 4, 4)).unwrap();
        assert!((m.info().duration - 0.2).abs() < 1e-9, "{delay} ms: {}", m.info().duration);
    }
    let m = open(testing::gif(&[(RED, 20), (GREEN, 20)], 4, 4)).unwrap();
    assert!((m.info().duration - 0.04).abs() < 1e-9);
}

#[test]
fn a_still_is_not_an_animation() {
    for bytes in [testing::gif(&[(RED, 100)], 4, 4), testing::apng(&[(RED, 100)], 4, 4)] {
        assert_eq!(open(bytes).unwrap_err(), MediaError::NotMedia);
    }
    let mut still = Vec::new();
    image::RgbaImage::from_pixel(3, 3, image::Rgba(RED)).write_to(&mut std::io::Cursor::new(&mut still), image::ImageFormat::Png).unwrap();
    assert_eq!(open(still).unwrap_err(), MediaError::NotMedia);
}

#[test]
fn long_animations_play_their_first_pictures_only() {
    let many: Vec<([u8; 4], u32)> = (0..MAX_ANIMATION_FRAMES + 5).map(|i| (if i % 2 == 0 { RED } else { GREEN }, 100)).collect();
    let m = open(testing::gif(&many, 1, 1)).unwrap();
    assert_eq!(m.info().frame_count as usize, MAX_ANIMATION_FRAMES);
    // A canvas so big that one picture is past the size limit is refused before anything is allocated.
    let mut huge = testing::gif(&[(RED, 100), (GREEN, 100)], 1, 1);
    huge[6..10].copy_from_slice(&[0x28, 0x23, 0x28, 0x23]);
    assert!(matches!(open(huge).unwrap_err(), MediaError::TooLarge(_) | MediaError::Corrupt(_)));
}

#[test]
fn damaged_animations_give_what_came_before_or_an_error() {
    let file = testing::gif(&[(RED, 100), (GREEN, 100), (BLUE, 100), (RED, 100)], 8, 8);
    let whole = open(file.clone()).unwrap();
    assert_eq!(whole.info().frame_count, 4);
    for n in 0..file.len() {
        if let Ok(m) = open(file[..n].to_vec()) {
            assert!(m.info().frame_count >= 2 && m.info().frame_count <= 4, "cut at {n}");
            let _ = m.frame_at(0.15).unwrap();
        }
    }
    let mut junk = b"GIF89a".to_vec();
    junk.extend((0..200u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8));
    assert!(open(junk).is_err());
}
