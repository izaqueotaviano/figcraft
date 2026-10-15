//! MP4 and MOV: QuickTime Motion-JPEG, MPEG-4 Visual objects of type JPEG, timing from the sample
//! tables, chunk layouts, box sizes, refused codecs and damaged files.

use std::sync::Arc;

use crate::testing::{self, BmffSpec, bmff_box, jpeg_picture, picture_level};
use crate::{Container, Media, MediaError, MediaKind};

fn open(bytes: Vec<u8>) -> Result<Media, MediaError> {
    Media::open(Arc::new(bytes))
}

fn level(m: &Media, t: f64) -> u8 {
    m.frame_at(t).unwrap().pixel(4, 4).unwrap()[0]
}

fn close(got: u8, index: u32) -> bool {
    got.abs_diff(picture_level(index)) < 14
}

#[test]
fn quicktime_motion_jpeg_opens() {
    for audio in [false, true] {
        let m = open(testing::mov_mjpeg(6, 48, 32, 12, audio)).unwrap();
        let i = m.info();
        assert_eq!((i.container, i.kind, i.codec.as_str()), (Container::Mov, MediaKind::Video, "Motion-JPEG"));
        assert_eq!((i.width, i.height, i.frame_count, i.has_audio), (48, 32, 6, audio));
        assert!((i.duration - 0.5).abs() < 1e-9 && (i.frame_rate - 12.0).abs() < 1e-9, "{i:?}");
        for k in 0..6 {
            assert!(close(level(&m, f64::from(k) / 12.0 + 0.002), k), "picture {k} (audio {audio})");
        }
    }
}

#[test]
fn mpeg_4_visual_of_the_jpeg_type_is_motion_jpeg_in_an_mp4() {
    let m = open(testing::mp4_mjpeg(5, 32, 24, 10)).unwrap();
    assert_eq!((m.info().container, m.info().frame_count), (Container::Mp4, 5));
    assert!((m.info().duration - 0.5).abs() < 1e-9);
    assert!(close(level(&m, 0.31), 3));
    // The same object with another type (0x20: MPEG-4 Visual proper) is that codec, refused.
    let jpegs: Vec<Vec<u8>> = (0..3).map(|i| jpeg_picture(32, 24, i)).collect();
    let dcd = [0x04, 13, 0x20, 0x11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut es = vec![0x03, (3 + dcd.len()) as u8, 0, 1, 0];
    es.extend_from_slice(&dcd);
    let spec = BmffSpec {
        brand: *b"isom",
        format: *b"mp4v",
        extra: bmff_box(b"esds", &[&[0u8; 4][..], &es].concat()),
        pictures: jpegs,
        ..BmffSpec::quicktime(3, 32, 24, 10)
    };
    let e = open(spec.bytes()).unwrap_err();
    assert_eq!(e, MediaError::UnsupportedCodec { container: Container::Mp4, codec: "MPEG-4 Visual (mp4v)".into() });
}

#[test]
fn other_codecs_are_refused_by_name() {
    let e = open(testing::mp4_h264(64, 48)).unwrap_err();
    assert_eq!(e, MediaError::UnsupportedCodec { container: Container::Mp4, codec: "H.264 (avc1)".into() });
    assert_eq!(e.to_string(), "This video codec isn't supported yet: H.264 (avc1)");
    let cases: [(&[u8; 4], &str); 10] = [
        (b"hvc1", "HEVC (hvc1)"),
        (b"hev1", "HEVC (hev1)"),
        (b"av01", "AV1 (av01)"),
        (b"vp09", "VP9 (vp09)"),
        (b"apch", "ProRes (apch)"),
        (b"mjpa", "Motion-JPEG with interlaced fields (mjpa)"),
        (b"s263", "H.263 (s263)"),
        (b"png ", "PNG (png )"),
        (b"SVQ3", "SVQ (SVQ3)"),
        (b"zzzz", "'zzzz'"),
    ];
    for (code, name) in cases {
        for brand in [b"qt  ", b"isom"] {
            let spec = BmffSpec {
                brand: *brand,
                format: *code,
                pictures: vec![vec![0, 1, 2, 3]; 3],
                durations: vec![10; 3],
                ..BmffSpec::quicktime(3, 32, 24, 10)
            };
            let want = if brand == b"qt  " { Container::Mov } else { Container::Mp4 };
            assert_eq!(open(spec.bytes()).unwrap_err(), MediaError::UnsupportedCodec { container: want, codec: name.into() });
        }
    }
}

#[test]
fn the_layout_of_chunks_and_boxes_does_not_matter() {
    let base = BmffSpec { audio: true, ..BmffSpec::quicktime(7, 32, 24, 10) };
    // (pictures per chunk, header first, 64-bit data box size)
    for (per_chunk, moov_first, large_mdat) in
        [(1, false, false), (3, false, false), (3, true, false), (7, true, true), (2, false, true), (100, true, false)]
    {
        let spec = BmffSpec { per_chunk, moov_first, large_mdat, ..base.clone() };
        let m = open(spec.bytes()).unwrap();
        assert_eq!(m.info().frame_count, 7, "{per_chunk} {moov_first} {large_mdat}");
        assert!(m.info().has_audio);
        for k in 0..7 {
            assert!(
                close(level(&m, f64::from(k) / 10.0 + 0.01), k),
                "picture {k} ({per_chunk} per chunk, moov first {moov_first}, large {large_mdat})"
            );
        }
    }
}

#[test]
fn pictures_start_when_the_sample_tables_say() {
    // 1/30 s, 1/15 s, 1/30 s, 1/30 s, 1/15 s, 1/30 s at 600 ticks a second.
    let spec = BmffSpec { durations: vec![20, 40, 20, 20, 40, 20], ..BmffSpec::quicktime(6, 32, 24, 30) };
    let m = open(spec.bytes()).unwrap();
    let want = [0.0, 20.0, 60.0, 80.0, 100.0, 140.0];
    for (i, w) in want.iter().enumerate() {
        assert!((m.frame_start(i as u32) - w / 600.0).abs() < 1e-9, "picture {i}: {}", m.frame_start(i as u32));
    }
    assert!((m.info().duration - 160.0 / 600.0).abs() < 1e-9);
    assert_eq!(m.frame_index_at(0.08), 1, "the long second picture covers 1/30 to 1/10");
    assert_eq!(m.frame_index_at(0.1), 2);
    // Durations that run out before the pictures do: the rest go on at the last pace.
    let short = BmffSpec { durations: vec![30, 30], ..BmffSpec::quicktime(5, 32, 24, 30) };
    let m = open(short.bytes()).unwrap();
    assert_eq!(m.info().frame_count, 5);
    assert!((m.info().duration - 150.0 / 600.0).abs() < 1e-9, "{}", m.info().duration);
}

#[test]
fn fragmented_files_and_files_without_a_header_are_refused() {
    let mut fragmented = testing::mov_mjpeg(3, 32, 24, 10, false);
    fragmented.extend_from_slice(&bmff_box(b"moof", &[0; 8]));
    assert!(matches!(open(fragmented).unwrap_err(), MediaError::Unsupported(m) if m.contains("fragmented")));
    // Only the data: the header (moov) is missing, as in a download cut short.
    let whole = testing::mov_mjpeg(3, 32, 24, 10, false);
    let moov = whole.windows(4).position(|w| w == b"moov").unwrap() - 4;
    let e = open(whole[..moov].to_vec()).unwrap_err();
    assert!(matches!(&e, MediaError::Corrupt(m) if m.contains("moov")), "{e:?}");
    // A header with no video track in it (an audio file).
    let mut audio = Vec::new();
    audio.extend_from_slice(&bmff_box(b"ftyp", b"M4A \0\0\0\0M4A "));
    audio.extend_from_slice(&bmff_box(b"moov", &bmff_box(b"mvhd", &[0; 100])));
    assert!(matches!(open(audio).unwrap_err(), MediaError::Unsupported(m) if m.contains("no video")));
}

#[test]
fn a_file_cut_inside_its_sample_data_keeps_the_pictures_it_has() {
    // The header (moov) is at the end of this layout; with it first, the data can be cut.
    let spec = BmffSpec { moov_first: true, ..BmffSpec::quicktime(6, 32, 24, 10) };
    let whole = spec.bytes();
    let m = open(whole.clone()).unwrap();
    assert_eq!(m.info().frame_count, 6);
    let cut = open(whole[..whole.len() - 300].to_vec()).unwrap();
    // The tables still list six; the pictures past the cut can't be decoded, the earlier ones can.
    assert!(close(level(&cut, 0.01), 0));
    assert!(cut.frame(5).is_err());
    assert!(cut.frame_at(0.55).is_ok(), "falls back to a picture before the cut");
}

#[test]
fn damaged_tables_are_errors_not_panics() {
    let whole = testing::mov_mjpeg(4, 32, 24, 10, true);
    for table in [&b"stsz"[..], b"stco", b"stts", b"stsc", b"stsd", b"mdhd", b"hdlr"] {
        let at = whole.windows(4).position(|w| w == table).unwrap();
        // Fill the box's payload with ones, zeros, and a ladder.
        for fill in [0xFFu8, 0x00, 0x7F] {
            let mut v = whole.clone();
            for b in v.iter_mut().skip(at + 4).take(24) {
                *b = fill;
            }
            if let Ok(m) = open(v) {
                let _ = m.frame_at(0.0);
                let _ = m.frame_at(m.info().duration);
            }
        }
    }
}
