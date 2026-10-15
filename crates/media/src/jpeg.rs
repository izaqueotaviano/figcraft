//! Motion-JPEG pictures: a JPEG per picture, decoded with the `image` crate's decoder after a
//! walk through its marker segments that checks them, and that supplies the standard Huffman tables
//! to pictures that leave them out (Motion-JPEG in AVI usually does: ITU-T T.81 Annex K.3).

use std::io::Cursor;
use std::sync::OnceLock;

use image::ImageDecoder;

use crate::bytes::{be16, slice};
use crate::{Frame, MAX_PIXELS, MAX_SIDE, MediaError, check_size, corrupt};

/// What a JPEG's headers say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JpegInfo {
    pub width: u32,
    pub height: u32,
    pub components: u8,
    /// A Huffman table (DHT) comes before the scan.
    pub has_dht: bool,
    /// Where the first scan's header (SOS) starts.
    pub sos: usize,
}

/// Walk the marker segments of `d` up to its first scan, checking every length.
pub(crate) fn scan(d: &[u8]) -> Result<JpegInfo, MediaError> {
    if slice(d, 0, 2) != Some(&[0xFF, 0xD8]) {
        return Err(corrupt("a picture isn't a JPEG"));
    }
    let mut p = 2usize;
    let mut size: Option<(u32, u32, u8)> = None;
    let mut has_dht = false;
    // A header is a few dozen segments; a hostile one can't make this loop long.
    for _ in 0..4096 {
        // Markers start with 0xFF, which may repeat as fill.
        while d.get(p) == Some(&0xFF) && d.get(p + 1) == Some(&0xFF) {
            p += 1;
        }
        if d.get(p) != Some(&0xFF) {
            return Err(corrupt("a JPEG segment starts where no marker is"));
        }
        let m = *d.get(p + 1).ok_or_else(|| corrupt("a JPEG ends inside its headers"))?;
        p += 2;
        match m {
            // Markers without a length: TEM, RSTn, SOI.
            0x00 | 0x01 | 0xD0..=0xD8 => continue,
            0xD9 => return Err(corrupt("a JPEG has no picture data")),
            _ => {}
        }
        let len = usize::from(be16(d, p).ok_or_else(|| corrupt("a JPEG ends inside its headers"))?);
        if len < 2 {
            return Err(corrupt("a JPEG segment is shorter than its length"));
        }
        let end = p.checked_add(len).filter(|e| *e <= d.len()).ok_or_else(|| corrupt("a JPEG segment runs past the end"))?;
        match m {
            0xC4 => has_dht = true,
            // Baseline, extended sequential and progressive Huffman: what the decoder reads.
            0xC0..=0xC2 => {
                let precision = *d.get(p + 2).ok_or_else(|| corrupt("a JPEG frame header is cut off"))?;
                let h = u32::from(be16(d, p + 3).ok_or_else(|| corrupt("a JPEG frame header is cut off"))?);
                let w = u32::from(be16(d, p + 5).ok_or_else(|| corrupt("a JPEG frame header is cut off"))?);
                let n = *d.get(p + 7).ok_or_else(|| corrupt("a JPEG frame header is cut off"))?;
                if precision != 8 {
                    return Err(MediaError::Unsupported("This JPEG video has more than 8 bits per sample, which isn't supported yet".into()));
                }
                if h == 0 {
                    return Err(MediaError::Unsupported("This JPEG video sets its height after the first scan, which isn't supported yet".into()));
                }
                if size.is_some() {
                    return Err(corrupt("a JPEG has two frame headers"));
                }
                size = Some((w, h, n));
            }
            // Lossless, hierarchical and arithmetic-coded JPEG.
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err(MediaError::Unsupported(
                    "This JPEG video uses a kind of JPEG that isn't supported yet (lossless, hierarchical or arithmetic)".into(),
                ));
            }
            0xDA => {
                let (width, height, components) = size.ok_or_else(|| corrupt("a JPEG has a scan before its frame header"))?;
                check_size(width, height)?;
                return Ok(JpegInfo { width, height, components, has_dht, sos: p - 2 });
            }
            _ => {}
        }
        p = end;
    }
    Err(corrupt("a JPEG has too many header segments"))
}

/// The standard tables of T.81 Annex K.3.3 (Tables K.3 to K.6), as one DHT segment with its marker.
pub(crate) fn standard_tables() -> &'static [u8] {
    // (class and id, code counts for lengths 1 to 16, symbols)
    const DC_LUMA: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
    const DC_CHROMA: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
    const DC_SYMBOLS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
    const AC_LUMA: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D];
    #[rustfmt::skip]
    const AC_LUMA_SYMBOLS: [u8; 162] = [
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
        0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0,
        0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28,
        0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
        0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
        0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
        0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7,
        0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5,
        0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2,
        0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8,
        0xF9, 0xFA,
    ];
    const AC_CHROMA: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
    #[rustfmt::skip]
    const AC_CHROMA_SYMBOLS: [u8; 162] = [
        0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
        0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33, 0x52, 0xF0,
        0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19, 0x1A, 0x26,
        0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
        0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
        0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
        0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
        0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3,
        0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA,
        0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8,
        0xF9, 0xFA,
    ];
    static TABLES: OnceLock<Vec<u8>> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut body = Vec::new();
        for (class_and_id, counts, symbols) in [
            (0x00u8, &DC_LUMA[..], &DC_SYMBOLS[..]),
            (0x10, &AC_LUMA[..], &AC_LUMA_SYMBOLS[..]),
            (0x01, &DC_CHROMA[..], &DC_SYMBOLS[..]),
            (0x11, &AC_CHROMA[..], &AC_CHROMA_SYMBOLS[..]),
        ] {
            body.push(class_and_id);
            body.extend_from_slice(counts);
            body.extend_from_slice(symbols);
        }
        let len = u16::try_from(body.len() + 2).unwrap_or(u16::MAX);
        let mut segment = vec![0xFF, 0xC4];
        segment.extend_from_slice(&len.to_be_bytes());
        segment.extend_from_slice(&body);
        segment
    })
}

/// `d` with the standard Huffman tables put in before its first scan (at `sos`).
pub(crate) fn with_standard_tables(d: &[u8], sos: usize) -> Vec<u8> {
    let tables = standard_tables();
    let (head, tail) = d.split_at(sos.min(d.len()));
    let mut out = Vec::with_capacity(d.len() + tables.len());
    out.extend_from_slice(head);
    out.extend_from_slice(tables);
    out.extend_from_slice(tail);
    out
}

/// Decode one JPEG picture to RGBA. `expect` is the size the video says its pictures have: a
/// picture of another size is refused (a damaged file could otherwise change sizes mid-video).
pub(crate) fn decode(d: &[u8], expect: Option<(u32, u32)>) -> Result<Frame, MediaError> {
    let info = scan(d)?;
    if let Some((w, h)) = expect
        && (info.width, info.height) != (w, h)
    {
        return Err(corrupt(format!("a picture is {} × {} in a video of {w} × {h}", info.width, info.height)));
    }
    let patched;
    let input = if info.has_dht {
        d
    } else {
        patched = with_standard_tables(d, info.sos);
        &patched
    };
    let mut decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(input)).map_err(|e| corrupt(e.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    decoder.set_limits(limits).map_err(|e| corrupt(e.to_string()))?;
    let (w, h) = decoder.dimensions();
    check_size(w, h)?;
    let img = image::DynamicImage::from_decoder(decoder).map_err(|e| corrupt(e.to_string()))?;
    let rgba = img.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(Frame { width, height, rgba: rgba.into_raw() })
}
