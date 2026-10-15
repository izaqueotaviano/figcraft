//! Hostile input. Every parser here is fed truncations, damaged words and random bytes of valid
//! files; the answer is a media file or an error, never a panic, and a file that opens keeps the
//! limits it promises.

use std::sync::Arc;

use proptest::prelude::*;

use crate::testing::{self, BmffSpec};
use crate::{MAX_DURATION, MAX_PIXELS, MAX_SIDE, Media, sniff};

/// Small valid files of every layout.
fn samples() -> Vec<(&'static str, Vec<u8>)> {
    let colors = [([255, 0, 0, 255], 100), ([0, 255, 0, 255], 100), ([0, 0, 255, 255], 50)];
    let spec = BmffSpec { audio: true, per_chunk: 2, ..BmffSpec::quicktime(4, 16, 16, 10) };
    vec![
        ("avi", testing::avi_mjpeg(4, 16, 16, 10, true, true)),
        ("avi without tables", testing::avi_mjpeg(4, 16, 16, 10, false, false)),
        ("mov", spec.bytes()),
        ("mov, header first", BmffSpec { moov_first: true, large_mdat: true, ..spec }.bytes()),
        ("mp4 with jpeg object type", testing::mp4_mjpeg(3, 16, 16, 10)),
        ("mp4 h264", testing::mp4_h264(16, 16)),
        ("gif", testing::gif(&colors, 8, 8)),
        ("apng", testing::apng(&colors, 8, 8)),
        ("webp", testing::webp_animation(&colors, 8, 8)),
        ("webm", testing::matroska("webm", "V_VP9")),
        ("synthetic", testing::synthetic(16, 16, 5, 5)),
    ]
}

/// What an opened file promises.
fn check(m: &Media) {
    let i = m.info();
    assert!(i.width > 0 && i.height > 0 && i.width <= MAX_SIDE && i.height <= MAX_SIDE, "{i:?}");
    assert!(u64::from(i.width) * u64::from(i.height) <= MAX_PIXELS, "{i:?}");
    assert!(i.duration.is_finite() && i.duration > 0.0 && i.duration <= MAX_DURATION, "{i:?}");
    assert!(i.frame_count >= 1 && i.frame_rate.is_finite() && i.frame_rate > 0.0, "{i:?}");
    for t in [-1.0, 0.0, i.duration / 2.0, i.duration, 1e12, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let index = m.frame_index_at(t);
        assert!(index < i.frame_count, "index {index} of {}", i.frame_count);
        let start = m.frame_start(index);
        assert!(start.is_finite() && (0.0..=i.duration).contains(&start), "{start}");
        if let Ok(f) = m.frame_at(t) {
            assert!(f.width > 0 && f.height > 0 && f.width <= MAX_SIDE && f.height <= MAX_SIDE);
            assert_eq!(f.rgba.len(), f.width as usize * f.height as usize * 4);
        }
    }
}

fn try_open(bytes: &[u8]) {
    let _ = sniff(bytes);
    if let Ok(m) = Media::open(Arc::new(bytes.to_vec())) {
        check(&m);
    }
}

#[test]
fn the_samples_themselves_are_fine() {
    for (name, bytes) in samples() {
        match Media::open(Arc::new(bytes)) {
            Ok(m) => check(&m),
            Err(e) => assert!(matches!(e, crate::MediaError::UnsupportedCodec { .. }), "{name}: {e}"),
        }
    }
}

#[test]
fn every_truncation_of_every_layout_is_an_answer() {
    for (_, bytes) in samples() {
        for n in 0..bytes.len() {
            try_open(&bytes[..n]);
        }
    }
}

#[test]
fn every_damaged_byte_of_every_layout_is_an_answer() {
    for (_, bytes) in samples() {
        for at in 0..bytes.len() {
            for flip in [0xFFu8, 0x80, 0x01] {
                let mut v = bytes.clone();
                v[at] ^= flip;
                try_open(&v);
            }
        }
    }
}

#[test]
fn damaged_words_claiming_the_biggest_numbers_are_an_answer() {
    for (_, bytes) in samples() {
        for at in (0..bytes.len().saturating_sub(4)).step_by(2) {
            for word in [[0xFF; 4], [0; 4], [0x7F, 0xFF, 0xFF, 0xFF], [0x80, 0, 0, 0], [0, 0, 0, 1]] {
                let mut v = bytes.clone();
                v[at..at + 4].copy_from_slice(&word);
                try_open(&v);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn random_bytes_are_an_answer(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        try_open(&bytes);
    }

    #[test]
    fn random_bytes_after_a_valid_start_are_an_answer(pick in 0usize..11, cut in 0usize..400, tail in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let (_, bytes) = samples().swap_remove(pick);
        let mut v = bytes[..cut.min(bytes.len())].to_vec();
        v.extend_from_slice(&tail);
        try_open(&v);
    }

    #[test]
    fn random_damage_to_valid_files_is_an_answer(pick in 0usize..11, hits in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..24)) {
        let (_, mut v) = samples().swap_remove(pick);
        for (at, byte) in hits {
            let n = v.len();
            if n > 0 {
                v[at % n] = byte;
            }
        }
        try_open(&v);
    }
}
