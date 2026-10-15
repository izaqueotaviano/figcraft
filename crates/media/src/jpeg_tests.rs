//! Motion-JPEG pictures: the marker walk, the standard Huffman tables for pictures that leave them
//! out, size checks and hostile input.

use crate::jpeg::{decode, scan, standard_tables, with_standard_tables};
use crate::testing::{jpeg_picture, picture_level, strip_tables};
use crate::{MAX_SIDE, MediaError};

/// The Huffman tables (DHT segments) of a JPEG as (class and id, code counts, symbols).
fn tables(jpeg: &[u8]) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    let mut p = 2;
    while p + 4 <= jpeg.len() && jpeg[p] == 0xFF {
        let m = jpeg[p + 1];
        let len = usize::from(jpeg[p + 2]) << 8 | usize::from(jpeg[p + 3]);
        if m == 0xDA {
            break;
        }
        if m == 0xC4 {
            let mut q = p + 4;
            while q < p + 2 + len {
                let class_and_id = jpeg[q];
                let counts = jpeg[q + 1..q + 17].to_vec();
                let n: usize = counts.iter().map(|c| usize::from(*c)).sum();
                out.push((class_and_id, counts, jpeg[q + 17..q + 17 + n].to_vec()));
                q += 17 + n;
            }
        }
        p += 2 + len;
    }
    out.sort();
    out
}

#[test]
fn the_standard_tables_are_those_of_annex_k() {
    // The `image` crate writes the standard tables (T.81 Annex K.3.3) into every JPEG it encodes:
    // ours must be the same four, byte for byte.
    let encoded = jpeg_picture(32, 24, 1);
    let ours = standard_tables();
    assert_eq!(&ours[..2], &[0xFF, 0xC4]);
    assert_eq!(usize::from(ours[2]) << 8 | usize::from(ours[3]), ours.len() - 2, "the segment's length");
    let mut with_marker = vec![0xFF, 0xD8];
    with_marker.extend_from_slice(ours);
    let theirs = tables(&encoded);
    assert_eq!(theirs.len(), 4);
    assert_eq!(tables(&with_marker), theirs);
}

#[test]
fn a_picture_without_tables_decodes_as_with_them() {
    for (w, h, i) in [(32, 24, 0), (48, 40, 3), (17, 9, 5), (64, 64, 8)] {
        let full = jpeg_picture(w, h, i);
        let bare = strip_tables(&full);
        assert!(bare.len() < full.len());
        let (a, b) = (scan(&full).unwrap(), scan(&bare).unwrap());
        assert!(a.has_dht && !b.has_dht);
        assert_eq!((a.width, a.height, a.components), (w, h, 3));
        assert_eq!((b.width, b.height), (w, h));
        let (fa, fb) = (decode(&full, Some((w, h))).unwrap(), decode(&bare, Some((w, h))).unwrap());
        assert_eq!(fa, fb, "{w}x{h}: the same pixels");
        assert_eq!((fa.width, fa.height, fa.rgba.len()), (w, h, (w * h * 4) as usize));
        // The grey block in the corner reads back within the JPEG's rounding.
        let [r, g, b, a] = fa.pixel(4, 4).unwrap();
        assert!(r.abs_diff(picture_level(i)) < 14 && g.abs_diff(r) < 8 && b.abs_diff(r) < 8 && a == 255, "{:?} vs {}", [r, g, b], picture_level(i));
    }
}

#[test]
fn the_tables_go_in_before_the_first_scan() {
    let bare = strip_tables(&jpeg_picture(16, 16, 0));
    let info = scan(&bare).unwrap();
    let patched = with_standard_tables(&bare, info.sos);
    assert_eq!(patched.len(), bare.len() + standard_tables().len());
    assert!(scan(&patched).unwrap().has_dht);
    // A position past the end is the end, not a panic.
    assert_eq!(with_standard_tables(&bare, usize::MAX).len(), bare.len() + standard_tables().len());
}

#[test]
fn the_walk_checks_what_it_reads() {
    let good = jpeg_picture(32, 24, 2);
    // Every prefix is an answer, never a panic; whole ones are fine and short ones aren't.
    for n in 0..good.len() {
        let _ = scan(&good[..n]);
        let _ = decode(&good[..n], None);
    }
    assert!(scan(&good).is_ok());
    assert!(matches!(scan(&[]), Err(MediaError::Corrupt(_))));
    assert!(matches!(scan(&[0xFF, 0xD8]), Err(MediaError::Corrupt(_))));
    assert!(matches!(scan(b"GIF89a"), Err(MediaError::Corrupt(_))));
    // End of image before any scan.
    assert!(matches!(scan(&[0xFF, 0xD8, 0xFF, 0xD9]), Err(MediaError::Corrupt(_))));
    // A segment whose length runs past the end, or is shorter than its own length field.
    assert!(scan(&[0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xFF, 0, 0]).is_err());
    assert!(scan(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x01, 0, 0]).is_err());
    // A scan before any frame header.
    assert!(scan(&[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02]).is_err());
    // Fill bytes before a marker are fine.
    let mut filled = vec![0xFF, 0xD8, 0xFF, 0xFF];
    filled.extend_from_slice(&good[2..]);
    assert_eq!(scan(&filled).unwrap().width, 32);
}

/// `good` with its frame header (SOF0) rewritten to claim `w` × `h`.
fn claims(good: &[u8], w: u16, h: u16) -> Vec<u8> {
    let mut v = good.to_vec();
    let at = v.windows(2).position(|p| p == [0xFF, 0xC0]).unwrap();
    v[at + 5..at + 7].copy_from_slice(&h.to_be_bytes());
    v[at + 7..at + 9].copy_from_slice(&w.to_be_bytes());
    v
}

#[test]
fn huge_claims_are_refused_before_anything_is_allocated() {
    let good = jpeg_picture(32, 24, 0);
    for (w, h) in [(65535, 65535), (u16::MAX, 1), (1, u16::MAX), (20_000, 20_000), (7000, 7000)] {
        let e = decode(&claims(&good, w, h), None).unwrap_err();
        assert!(matches!(e, MediaError::TooLarge(_)), "{w}x{h}: {e:?}");
    }
    assert!(u32::from(u16::MAX) > MAX_SIDE);
    // No size, or a height left to a later marker.
    assert!(scan(&claims(&good, 0, 24)).is_err());
    assert!(matches!(scan(&claims(&good, 32, 0)), Err(MediaError::Unsupported(_))));
    // A picture of another size than the video's is refused.
    assert!(matches!(decode(&good, Some((64, 48))), Err(MediaError::Corrupt(_))));
    assert!(decode(&good, Some((32, 24))).is_ok());
}

#[test]
fn kinds_of_jpeg_the_decoder_does_not_read_are_refused_by_name() {
    let good = jpeg_picture(16, 16, 0);
    let at = good.windows(2).position(|p| p == [0xFF, 0xC0]).unwrap();
    // Arithmetic coding (SOF9), lossless (SOF3) and 12-bit samples.
    for (marker, what) in [(0xC9u8, "arithmetic"), (0xC3, "lossless")] {
        let mut v = good.clone();
        v[at + 1] = marker;
        assert!(matches!(scan(&v), Err(MediaError::Unsupported(m)) if m.contains("kind of JPEG")), "{what}");
    }
    let mut v = good.clone();
    v[at + 4] = 12;
    assert!(matches!(scan(&v), Err(MediaError::Unsupported(m)) if m.contains("8 bits")));
}
