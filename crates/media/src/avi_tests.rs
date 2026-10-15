//! AVI: Motion-JPEG with and without Huffman tables, timing, audio streams, other codecs refused
//! by name, damaged and cut-short files, OpenDML continuations.

use std::sync::Arc;

use crate::testing::{self, avi, jpeg_picture, picture_level, riff, riff_list};
use crate::{Container, Media, MediaError, MediaKind};

fn open(bytes: Vec<u8>) -> Result<Media, MediaError> {
    Media::open(Arc::new(bytes))
}

/// The grey level of the top-left block of the picture showing at `t`.
fn level(m: &Media, t: f64) -> u8 {
    m.frame_at(t).unwrap().pixel(4, 4).unwrap()[0]
}

fn close(got: u8, index: u32) -> bool {
    got.abs_diff(picture_level(index)) < 14
}

#[test]
fn motion_jpeg_opens_with_its_tables_or_without_them() {
    for tables in [true, false] {
        for audio in [false, true] {
            let m = open(testing::avi_mjpeg(6, 48, 32, 12, tables, audio)).unwrap();
            let i = m.info();
            assert_eq!((i.container, i.kind, i.codec.as_str()), (Container::Avi, MediaKind::Video, "Motion-JPEG"));
            assert_eq!((i.width, i.height, i.frame_count, i.has_audio), (48, 32, 6, audio), "tables {tables}");
            assert!((i.duration - 0.5).abs() < 1e-9 && (i.frame_rate - 12.0).abs() < 1e-9, "{i:?}");
            // Each picture at its time (and halfway through it).
            for k in 0..6 {
                let t = f64::from(k) / 12.0;
                assert!(close(level(&m, t + 0.001), k), "picture {k} at {t} (tables {tables}, audio {audio})");
                assert_eq!(m.frame_index_at(t + 0.04), k);
            }
            assert_eq!(m.frame_index_at(0.5), 5, "at the end: the last");
        }
    }
}

#[test]
fn other_codecs_are_refused_by_name() {
    // (the codec code the file carries, the name the refusal gives)
    let cases: [(&[u8; 4], &str); 9] = [
        (b"H264", "H.264 (H264)"),
        (b"avc1", "H.264 (avc1)"),
        (b"HEVC", "HEVC (HEVC)"),
        (b"XVID", "MPEG-4 Visual (XVID)"),
        (b"WMV3", "WMV (WMV3)"),
        (b"VP90", "VP9 (VP90)"),
        (b"MJPA", "Motion-JPEG with interlaced fields (MJPA)"),
        (b"zzzz", "'zzzz'"),
        (b"\x01\x02ab", "'??ab'"),
    ];
    for (code, name) in cases {
        let e = open(avi(code, &vec![vec![1, 2, 3]; 3], 32, 24, 10, false)).unwrap_err();
        assert_eq!(e, MediaError::UnsupportedCodec { container: Container::Avi, codec: name.into() }, "{}", String::from_utf8_lossy(code));
        assert_eq!(e.to_string(), format!("This video codec isn't supported yet: {name}"));
    }
}

#[test]
fn a_file_cut_short_keeps_the_pictures_before_the_cut() {
    let whole = testing::avi_mjpeg(6, 32, 24, 12, true, false);
    let m = open(whole.clone()).unwrap();
    assert_eq!(m.info().frame_count, 6);
    // Cut inside the fourth picture's data: three whole pictures remain.
    let fourth = whole.windows(4).enumerate().filter(|(_, w)| w == b"00dc").map(|(i, _)| i).nth(3).unwrap();
    let cut = open(whole[..fourth + 20].to_vec()).unwrap();
    assert_eq!(cut.info().frame_count, 3);
    assert!(close(level(&cut, 0.26), 2));
    // Cut in the middle of the headers: nothing to play.
    assert!(open(whole[..120].to_vec()).is_err());
}

#[test]
fn a_chunk_without_data_is_a_dropped_picture_and_the_one_before_shows_again() {
    let mut pictures: Vec<Vec<u8>> = (0..4).map(|i| jpeg_picture(32, 24, i)).collect();
    pictures[2] = Vec::new();
    let m = open(avi(b"MJPG", &pictures, 32, 24, 4, false)).unwrap();
    assert_eq!(m.info().frame_count, 4);
    assert_eq!(*m.frame(2).unwrap(), *m.frame(1).unwrap());
    assert!(close(level(&m, 0.80), 3));
    // Everything empty is no video.
    let e = open(avi(b"MJPG", &[Vec::new(), Vec::new()], 32, 24, 4, false)).unwrap_err();
    assert!(matches!(e, MediaError::Corrupt(_)), "{e:?}");
}

#[test]
fn opendml_continuations_add_their_pictures() {
    let first = testing::avi_mjpeg(3, 32, 24, 6, true, false);
    // A second RIFF chunk of form AVIX with a movi list of three more pictures.
    let more: Vec<Vec<u8>> = (3..6).map(|i| riff(b"00dc", &jpeg_picture(32, 24, i))).collect();
    let mut body = b"AVIX".to_vec();
    body.extend_from_slice(&riff_list(b"movi", &more));
    let mut file = first;
    file.extend_from_slice(&riff(b"RIFF", &body));
    let m = open(file).unwrap();
    assert_eq!(m.info().frame_count, 6);
    assert!((m.info().duration - 1.0).abs() < 1e-9);
    for k in 0..6 {
        assert!(close(level(&m, f64::from(k) / 6.0 + 0.01), k), "picture {k}");
    }
}

#[test]
fn a_picture_of_another_size_is_not_shown() {
    let pictures = vec![jpeg_picture(64, 32, 0), jpeg_picture(32, 32, 1), jpeg_picture(64, 32, 2)];
    let m = open(avi(b"MJPG", &pictures, 64, 32, 3, false)).unwrap();
    assert_eq!((m.info().width, m.info().height), (64, 32));
    assert!(matches!(m.frame(1), Err(MediaError::Corrupt(_))));
    // The picture showing at 0.4 s is the second; the first stands in for it.
    let f = m.frame_at(0.4).unwrap();
    assert_eq!((f.width, f.height), (64, 32));
    assert!(close(f.pixel(4, 4).unwrap()[0], 0));
    assert!(m.frame(2).is_ok());
}

#[test]
fn the_files_headers_are_a_fallback_for_timing_only() {
    // A stream header with a zero rate falls back to the main header's microseconds per frame.
    let mut file = testing::avi_mjpeg(4, 32, 24, 8, true, false);
    // strh: after "strh" and its size, the type and handler (8), flags, priority (8), initial frames (4), scale (4), rate.
    let at = file.windows(4).position(|w| w == b"strh").unwrap();
    let rate = at + 8 + 8 + 4 + 4 + 4 + 4;
    file[rate..rate + 4].copy_from_slice(&0u32.to_le_bytes());
    let m = open(file.clone()).unwrap();
    assert!((m.info().duration - 0.5).abs() < 1e-9, "8 fps from the main header: {}", m.info().duration);
    // Neither: no timing.
    let avih = file.windows(4).position(|w| w == b"avih").unwrap();
    file[avih + 8..avih + 12].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(open(file).unwrap_err(), MediaError::Corrupt(_)));
}

#[test]
fn a_file_without_a_video_stream_or_with_junk_inside_is_an_error_not_a_panic() {
    assert!(matches!(open(avi(b"MJPG", &[], 32, 24, 8, false)).unwrap_err(), MediaError::Corrupt(_)));
    // RIFF chunks that claim more than the file has, nested lists to a depth no real file has, and
    // an empty body.
    let mut deep = Vec::new();
    for _ in 0..50 {
        deep = riff_list(b"hdrl", &[deep]);
    }
    let mut body = b"AVI ".to_vec();
    body.extend_from_slice(&deep);
    for junk in [riff(b"RIFF", &body), riff(b"RIFF", b"AVI "), b"RIFF\xff\xff\xff\xffAVI LIST\xff\xff\xff\xffhdrl".to_vec()] {
        assert!(open(junk).is_err());
    }
}
