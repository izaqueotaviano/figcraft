//! Test builders (feature `testing`): a synthetic source and writers of small video files, so
//! everything built on [`Media`](crate::Media) is tested without real files.
//!
//! The **synthetic source** is a few bytes ([`synthetic`]) that open as a video of any size, length
//! and rate whose pictures show their own index: the top-left 8 × 8 pixels of picture `i` are
//! `(i & 255, i >> 8, 128)` ([`frame_index`] reads it back), under a gradient and a bar that
//! moves with the index. The **writers** make real container files from JPEG pictures
//! ([`jpeg_picture`]): [`avi`], [`mov`], [`mp4`] (their codec code is a parameter, to make files
//! in codecs that aren't decoded), [`gif`], [`apng`], [`webp_animation`] and [`matroska`].

use std::io::Cursor;

use image::ExtendedColorType;

use crate::{Frame, Media, MediaError, MediaInfo, MediaKind, Source, check_size};

/// What a synthetic source's bytes start with.
pub const SYNTHETIC_MAGIC: &[u8; 8] = b"FCSYNTH1";

/// A synthetic video's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntheticSpec {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    /// Pictures per second.
    pub fps: u32,
    pub audio: bool,
}

impl SyntheticSpec {
    pub fn seconds(self) -> f64 {
        f64::from(self.frames) / f64::from(self.fps.max(1))
    }

    /// The bytes that open as this video.
    pub fn bytes(self) -> Vec<u8> {
        let mut b = SYNTHETIC_MAGIC.to_vec();
        for v in [self.width, self.height, self.frames, self.fps, u32::from(self.audio)] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }
}

/// The bytes of a synthetic video of `frames` pictures at `fps` pictures per second.
pub fn synthetic(width: u32, height: u32, frames: u32, fps: u32) -> Vec<u8> {
    SyntheticSpec { width, height, frames, fps, audio: false }.bytes()
}

pub(crate) fn is_synthetic(b: &[u8]) -> bool {
    b.starts_with(SYNTHETIC_MAGIC)
}

fn parse(b: &[u8]) -> Option<SyntheticSpec> {
    let n = |i: usize| crate::bytes::le32(b, 8 + 4 * i);
    Some(SyntheticSpec { width: n(0)?, height: n(1)?, frames: n(2)?, fps: n(3)?, audio: n(4)? != 0 })
}

pub(crate) fn open(b: &[u8]) -> Result<Media, MediaError> {
    let s = parse(b).ok_or(MediaError::NotMedia)?;
    check_size(s.width, s.height)?;
    if s.frames == 0 || s.fps == 0 || s.frames as usize > crate::MAX_FRAMES {
        return Err(crate::corrupt("a synthetic video needs pictures and a rate"));
    }
    let starts: Vec<f64> = (0..s.frames).map(|i| f64::from(i) / f64::from(s.fps)).collect();
    let info = MediaInfo {
        container: crate::Container::Synthetic,
        kind: MediaKind::Video,
        codec: "synthetic".into(),
        width: s.width,
        height: s.height,
        duration: s.seconds(),
        frame_count: s.frames,
        frame_rate: f64::from(s.fps),
        has_audio: s.audio,
    };
    Ok(Media::new(info, starts, Source::Synthetic(s)))
}

/// Picture `index` of a synthetic video.
pub(crate) fn frame(s: &SyntheticSpec, index: u32) -> Frame {
    let (w, h) = (s.width.max(1), s.height.max(1));
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    let bar = (u64::from(index) * 5 % u64::from(w)) as u32;
    for y in 0..h {
        for x in 0..w {
            let px = if x < 8 && y < 8 {
                [(index & 0xFF) as u8, ((index >> 8) & 0xFF) as u8, 0x80, 255]
            } else if x >= bar && x < bar.saturating_add(6) {
                [255, 255, 255, 255]
            } else {
                [(x * 255 / w) as u8, (y * 255 / h) as u8, 64, 255]
            };
            rgba.extend_from_slice(&px);
        }
    }
    Frame { width: w, height: h, rgba }
}

/// The index of a synthetic picture, read from its top-left pixel.
pub fn frame_index(f: &Frame) -> Option<u32> {
    let [r, g, b, _] = f.pixel(0, 0)?;
    (b == 0x80).then(|| u32::from(r) | u32::from(g) << 8)
}

// ---- JPEG pictures ------------------------------------------------------------------------------

/// The grey level a [`jpeg_picture`]'s top-left block has: it grows with the index (readable after
/// the JPEG's rounding, within ±12), and wraps after nine pictures.
pub fn picture_level(index: u32) -> u8 {
    20 + 25 * (index % 9) as u8
}

/// A baseline JPEG picture of `w` × `h` for picture `index`: a gradient, a bar that moves with the
/// index and a 16 × 16 block of [`picture_level`] grey in the top-left corner.
pub fn jpeg_picture(w: u32, h: u32, index: u32) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    let bar = (u64::from(index) * 8 % u64::from(w.max(1))) as u32;
    for y in 0..h {
        for x in 0..w {
            let px = if x < 16 && y < 16 {
                let l = picture_level(index);
                [l, l, l]
            } else if x >= bar && x < bar.saturating_add(8) {
                [250, 250, 250]
            } else {
                [(x * 200 / w.max(1)) as u8, (y * 200 / h.max(1)) as u8, 90]
            };
            rgb.extend_from_slice(&px);
        }
    }
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
    // (A synthetic picture always encodes; an empty result would only make a test fail.)
    let _ = enc.encode(&rgb, w, h, ExtendedColorType::Rgb8);
    out
}

/// `jpeg` without its Huffman tables (DHT segments), as Motion-JPEG in AVI usually is.
pub fn strip_tables(jpeg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(jpeg.len());
    let mut p = 2usize;
    out.extend_from_slice(jpeg.get(..2).unwrap_or_default());
    while p + 4 <= jpeg.len() {
        let (Some(0xFF), Some(&m)) = (jpeg.get(p), jpeg.get(p + 1)) else { break };
        let len = usize::from(jpeg.get(p + 2).copied().unwrap_or(0)) << 8 | usize::from(jpeg.get(p + 3).copied().unwrap_or(0));
        if m == 0xDA {
            break;
        }
        if m != 0xC4 {
            out.extend_from_slice(jpeg.get(p..p + 2 + len).unwrap_or_default());
        }
        p += 2 + len;
    }
    out.extend_from_slice(jpeg.get(p..).unwrap_or_default());
    out
}

// ---- byte helpers --------------------------------------------------------------------------------

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// A RIFF chunk: id, little-endian size, payload, a pad byte for an odd size.
pub(crate) fn riff(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend_from_slice(&le32(payload.len() as u32));
    v.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        v.push(0);
    }
    v
}

/// A RIFF list: `LIST`, its size, the list type and the chunks.
pub(crate) fn riff_list(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut body = kind.to_vec();
    chunks.iter().for_each(|c| body.extend_from_slice(c));
    riff(b"LIST", &body)
}

/// A box of the ISO base media file format: big-endian size, type, payload.
pub(crate) fn bmff_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = be32(8 + payload.len() as u32).to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

/// A full box (version and flags first).
fn full_box(kind: &[u8; 4], version: u8, payload: &[u8]) -> Vec<u8> {
    let mut p = vec![version, 0, 0, 0];
    p.extend_from_slice(payload);
    bmff_box(kind, &p)
}

fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.iter().flatten().copied().collect()
}

// ---- AVI -------------------------------------------------------------------------------------------

/// An AVI file of `pictures` (already encoded in the codec `fourcc` names: `MJPG` for JPEGs) at
/// `fps` pictures per second, with an audio stream of silence when `audio`.
pub fn avi(fourcc: &[u8; 4], pictures: &[Vec<u8>], w: u32, h: u32, fps: u32, audio: bool) -> Vec<u8> {
    let n = pictures.len() as u32;
    let mut avih = Vec::new();
    for v in [1_000_000 / fps.max(1), 0, 0, 0x10, n, 0, if audio { 2 } else { 1 }, 1 << 20, w, h, 0, 0, 0, 0] {
        avih.extend_from_slice(&le32(v));
    }
    let mut strh = b"vids".to_vec();
    strh.extend_from_slice(fourcc);
    for v in [0, 0, 0, 1, fps, 0, n, 1 << 20, 0xFFFF_FFFF, 0] {
        strh.extend_from_slice(&le32(v));
    }
    strh.extend_from_slice(&[0, 0, 0, 0, w as u8, (w >> 8) as u8, h as u8, (h >> 8) as u8]);
    let mut strf = Vec::new();
    for v in [40, w, h] {
        strf.extend_from_slice(&le32(v));
    }
    strf.extend_from_slice(&[1, 0, 24, 0]);
    strf.extend_from_slice(fourcc);
    for v in [w * h * 3, 0, 0, 0, 0] {
        strf.extend_from_slice(&le32(v));
    }
    let mut streams = vec![riff_list(b"strl", &[riff(b"strh", &strh), riff(b"strf", &strf)])];
    if audio {
        let mut ash = b"auds".to_vec();
        ash.extend_from_slice(&[0; 4]);
        for v in [0, 0, 0, 1, 8000, 0, n * 800, 8000, 0xFFFF_FFFF, 1] {
            ash.extend_from_slice(&le32(v));
        }
        ash.extend_from_slice(&[0; 8]);
        // WAVEFORMATEX: PCM, mono, 8000 Hz, 8 bits.
        let wf = [1, 0, 1, 0, 0x40, 0x1F, 0, 0, 0x40, 0x1F, 0, 0, 1, 0, 8, 0, 0, 0];
        streams.push(riff_list(b"strl", &[riff(b"strh", &ash), riff(b"strf", &wf)]));
    }
    let hdrl = riff_list(b"hdrl", &[vec![riff(b"avih", &avih)], streams].concat());
    let mut movi_chunks = Vec::new();
    let mut index = Vec::new();
    let mut offset = 4u32;
    for p in pictures {
        let c = riff(b"00dc", p);
        index.extend_from_slice(b"00dc");
        for v in [0x10, offset, p.len() as u32] {
            index.extend_from_slice(&le32(v));
        }
        offset += c.len() as u32;
        movi_chunks.push(c);
        if audio {
            let a = riff(b"01wb", &vec![128u8; 800]);
            index.extend_from_slice(b"01wb");
            for v in [0, offset, 800] {
                index.extend_from_slice(&le32(v));
            }
            offset += a.len() as u32;
            movi_chunks.push(a);
        }
    }
    let movi = riff_list(b"movi", &movi_chunks);
    let mut body = b"AVI ".to_vec();
    for part in [hdrl, riff(b"JUNK", &[0; 6]), movi, riff(b"idx1", &index)] {
        body.extend_from_slice(&part);
    }
    riff(b"RIFF", &body)
}

/// An AVI of Motion-JPEG pictures, each with its Huffman tables (`tables`) or without (as most
/// cameras and tools write them).
pub fn avi_mjpeg(pictures: u32, w: u32, h: u32, fps: u32, tables: bool, audio: bool) -> Vec<u8> {
    let jpegs: Vec<Vec<u8>> = (0..pictures).map(|i| jpeg_picture(w, h, i)).map(|j| if tables { j } else { strip_tables(&j) }).collect();
    avi(b"MJPG", &jpegs, w, h, fps, audio)
}

// ---- MP4 / MOV ---------------------------------------------------------------------------------------

/// One track's tables for [`bmff`].
struct Tables {
    format: [u8; 4],
    extra: Vec<u8>,
    sizes: Vec<u32>,
    durations: Vec<u32>,
    /// One offset per chunk.
    offsets: Vec<u32>,
    /// Samples in each chunk but the last.
    per_chunk: u32,
}

fn visual_entry(format: &[u8; 4], w: u32, h: u32, extra: &[u8]) -> Vec<u8> {
    let mut e = vec![0; 6];
    e.extend_from_slice(&[0, 1]);
    e.extend_from_slice(&[0; 16]);
    e.extend_from_slice(&[(w >> 8) as u8, w as u8, (h >> 8) as u8, h as u8]);
    e.extend_from_slice(&[0, 0x48, 0, 0, 0, 0x48, 0, 0]);
    e.extend_from_slice(&[0; 4]);
    e.extend_from_slice(&[0, 1]);
    e.extend_from_slice(&[0; 32]);
    e.extend_from_slice(&[0, 24, 0xFF, 0xFF]);
    e.extend_from_slice(extra);
    bmff_box(format, &e)
}

fn track(handler: &[u8; 4], timescale: u32, t: &Tables, w: u32, h: u32) -> Vec<u8> {
    let total: u32 = t.durations.iter().sum();
    let entry = if handler == b"vide" {
        visual_entry(&t.format, w, h, &t.extra)
    } else {
        // An audio sample entry: reserved bytes, a data reference, 8 reserved bytes, then 1
        // channel, 8 bits, and the rate 8000.0.
        let mut e = vec![0; 6];
        e.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 8, 0, 0, 0, 0, 0x1F, 0x40, 0, 0]);
        bmff_box(&t.format, &e)
    };
    let stsd = full_box(b"stsd", 0, &[&be32(1)[..], &entry].concat());
    // (A constant-rate video is one entry; others list each run of equal durations.)
    let runs: Vec<(u32, u32)> = t.durations.iter().fold(Vec::new(), |mut v: Vec<(u32, u32)>, d| {
        match v.last_mut() {
            Some(last) if last.1 == *d => last.0 += 1,
            _ => v.push((1, *d)),
        }
        v
    });
    let rows: Vec<u8> = runs.iter().flat_map(|(c, d)| [be32(*c), be32(*d)].concat()).collect();
    let stts = full_box(b"stts", 0, &[&be32(runs.len() as u32)[..], &rows].concat());
    // Chunks of `per_chunk` samples, the last holding the rest.
    let per = t.per_chunk.max(1);
    let chunks = t.offsets.len() as u32;
    let last = (t.sizes.len() as u32).saturating_sub(per * chunks.saturating_sub(1));
    let mut stsc_rows = [be32(1), be32(per), be32(1)].concat();
    let mut entries = 1;
    if chunks > 1 && last != per {
        stsc_rows.extend_from_slice(&[be32(chunks), be32(last), be32(1)].concat());
        entries = 2;
    }
    let stsc = full_box(b"stsc", 0, &[&be32(entries)[..], &stsc_rows].concat());
    let mut sz = be32(0).to_vec();
    sz.extend_from_slice(&be32(t.sizes.len() as u32));
    t.sizes.iter().for_each(|s| sz.extend_from_slice(&be32(*s)));
    let stsz = full_box(b"stsz", 0, &sz);
    let mut co = be32(t.offsets.len() as u32).to_vec();
    t.offsets.iter().for_each(|o| co.extend_from_slice(&be32(*o)));
    let stco = full_box(b"stco", 0, &co);
    let stbl = bmff_box(b"stbl", &cat(&[stsd, stts, stsc, stsz, stco]));
    let media_header = if handler == b"vide" { full_box(b"vmhd", 0, &[0; 8]) } else { full_box(b"smhd", 0, &[0; 4]) };
    let dinf = bmff_box(b"dinf", &full_box(b"dref", 0, &[&be32(1)[..], &full_box(b"url ", 0, &[])].concat()));
    let minf = bmff_box(b"minf", &cat(&[media_header, dinf, stbl]));
    let mut mdhd = vec![0; 8];
    mdhd.extend_from_slice(&be32(timescale));
    mdhd.extend_from_slice(&be32(total));
    mdhd.extend_from_slice(&[0x55, 0xC4, 0, 0]);
    let mut hdlr = vec![0; 4];
    hdlr.extend_from_slice(handler);
    hdlr.extend_from_slice(&[0; 12]);
    hdlr.push(0);
    let mdia = bmff_box(b"mdia", &cat(&[full_box(b"mdhd", 0, &mdhd), full_box(b"hdlr", 0, &hdlr), minf]));
    let mut tkhd = vec![0; 8];
    tkhd.extend_from_slice(&be32(if handler == b"vide" { 1 } else { 2 }));
    tkhd.extend_from_slice(&[0; 4]);
    tkhd.extend_from_slice(&be32(total));
    // Reserved, layer, group, volume, reserved, and the identity matrix.
    tkhd.extend_from_slice(&[0; 16]);
    for v in [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
        tkhd.extend_from_slice(&be32(v));
    }
    tkhd.extend_from_slice(&be32(w << 16));
    tkhd.extend_from_slice(&be32(h << 16));
    bmff_box(b"trak", &cat(&[full_box(b"tkhd", 0, &tkhd), mdia]))
}

/// A MOV or MP4 file to write.
#[derive(Clone, Debug)]
pub struct BmffSpec {
    /// `qt  ` for QuickTime, `isom` for MP4.
    pub brand: [u8; 4],
    /// The video sample description's code: `jpeg` for QuickTime's JPEG; `mp4v`, `avc1`… to
    /// make files in codecs that aren't decoded.
    pub format: [u8; 4],
    /// Boxes that follow the video sample description's header (`esds`…).
    pub extra: Vec<u8>,
    pub pictures: Vec<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub timescale: u32,
    /// Ticks each picture lasts.
    pub durations: Vec<u32>,
    /// A silent audio track.
    pub audio: bool,
    /// Pictures per chunk (1 is the simplest layout; real files group several).
    pub per_chunk: u32,
    /// The `moov` box before the sample data ("fast start"), else after it.
    pub moov_first: bool,
    /// The sample data's box with a 64-bit size field.
    pub large_mdat: bool,
}

impl BmffSpec {
    /// Motion-JPEG pictures in QuickTime's `jpeg`, at `fps`.
    pub fn quicktime(pictures: u32, w: u32, h: u32, fps: u32) -> BmffSpec {
        BmffSpec {
            brand: *b"qt  ",
            format: *b"jpeg",
            extra: Vec::new(),
            pictures: (0..pictures).map(|i| jpeg_picture(w, h, i)).collect(),
            width: w,
            height: h,
            timescale: 600,
            durations: vec![600 / fps.max(1); pictures as usize],
            audio: false,
            per_chunk: 1,
            moov_first: false,
            large_mdat: false,
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        bmff(self)
    }
}

/// The file `spec` describes.
pub fn bmff(spec: &BmffSpec) -> Vec<u8> {
    let ftyp = bmff_box(b"ftyp", &cat(&[spec.brand.to_vec(), be32(0).to_vec(), spec.brand.to_vec()]));
    let per = spec.per_chunk.max(1) as usize;
    // The sample data: chunks of `per` pictures, each followed by the audio that goes with it.
    let mut data = Vec::new();
    let mut video = Tables {
        format: spec.format,
        extra: spec.extra.clone(),
        sizes: vec![],
        durations: spec.durations.clone(),
        offsets: vec![],
        per_chunk: per as u32,
    };
    let mut sound = Tables { format: *b"sowt", extra: vec![], sizes: vec![], durations: vec![], offsets: vec![], per_chunk: per as u32 };
    let mut chunk_starts: Vec<(usize, usize)> = Vec::new();
    for (c, group) in spec.pictures.chunks(per).enumerate() {
        chunk_starts.push((data.len(), 0));
        for p in group {
            video.sizes.push(p.len() as u32);
            data.extend_from_slice(p);
        }
        if spec.audio {
            let at = data.len();
            for (i, _) in group.iter().enumerate() {
                sound.sizes.push(400);
                sound.durations.push(spec.durations.get(c * per + i).copied().unwrap_or(1));
                data.extend_from_slice(&[128; 400]);
            }
            chunk_starts.push((at, 1));
        }
    }
    let total: u32 = spec.durations.iter().sum();
    let mut mvhd = vec![0; 8];
    mvhd.extend_from_slice(&be32(spec.timescale));
    mvhd.extend_from_slice(&be32(total));
    mvhd.extend_from_slice(&be32(0x0001_0000));
    // Volume 1.0, then 10 reserved bytes.
    mvhd.extend_from_slice(&[1, 0, 0, 0]);
    mvhd.extend_from_slice(&[0; 8]);
    for v in [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
        mvhd.extend_from_slice(&be32(v));
    }
    mvhd.extend_from_slice(&[0; 24]);
    mvhd.extend_from_slice(&be32(3));
    // The moov box holds absolute offsets into the file, which depend on its own size when it
    // comes first: build it once to learn the size, then again with the offsets.
    let build = |base: u32| -> Vec<u8> {
        let v = with_offsets(&video, &chunk_starts, 0, base);
        let mut traks = vec![track(b"vide", spec.timescale, &v, spec.width, spec.height)];
        if spec.audio {
            traks.push(track(b"soun", spec.timescale, &with_offsets(&sound, &chunk_starts, 1, base), 0, 0));
        }
        bmff_box(b"moov", &[full_box(b"mvhd", 0, &mvhd), cat(&traks)].concat())
    };
    let mdat_header = if spec.large_mdat { 16 } else { 8 };
    let mdat = |data: &[u8]| -> Vec<u8> {
        if spec.large_mdat {
            let mut v = be32(1).to_vec();
            v.extend_from_slice(b"mdat");
            v.extend_from_slice(&(16 + data.len() as u64).to_be_bytes());
            v.extend_from_slice(data);
            v
        } else {
            bmff_box(b"mdat", data)
        }
    };
    if spec.moov_first {
        let size = build(0).len() as u32;
        let moov = build(ftyp.len() as u32 + size + mdat_header);
        cat(&[ftyp.clone(), moov, mdat(&data)])
    } else {
        let moov = build(ftyp.len() as u32 + mdat_header);
        cat(&[ftyp.clone(), mdat(&data), moov])
    }
}

/// A track's tables with the offsets of its chunks (`kind` 0: video, 1: audio) for sample data
/// that starts at `base` in the file.
fn with_offsets(t: &Tables, chunk_starts: &[(usize, usize)], kind: usize, base: u32) -> Tables {
    let offsets = chunk_starts.iter().filter(|c| c.1 == kind).map(|c| base + c.0 as u32).collect();
    Tables { format: t.format, extra: t.extra.clone(), sizes: t.sizes.clone(), durations: t.durations.clone(), offsets, per_chunk: t.per_chunk }
}

/// A QuickTime movie of Motion-JPEG pictures (`jpeg`).
pub fn mov_mjpeg(pictures: u32, w: u32, h: u32, fps: u32, audio: bool) -> Vec<u8> {
    BmffSpec { audio, ..BmffSpec::quicktime(pictures, w, h, fps) }.bytes()
}

/// An MP4 whose video is Motion-JPEG in MPEG-4 Visual's clothes (`mp4v` with object type 0x6C).
pub fn mp4_mjpeg(pictures: u32, w: u32, h: u32, fps: u32) -> Vec<u8> {
    // esds: the ES descriptor (tag 3) holding the decoder configuration descriptor (tag 4) of object type 0x6C.
    let dcd = [0x04, 13, 0x6C, 0x11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut es = vec![0x03, (3 + dcd.len()) as u8, 0, 1, 0];
    es.extend_from_slice(&dcd);
    BmffSpec {
        brand: *b"isom",
        format: *b"mp4v",
        extra: full_box(b"esds", 0, &es),
        timescale: 1000,
        durations: vec![1000 / fps.max(1); pictures as usize],
        ..BmffSpec::quicktime(pictures, w, h, fps)
    }
    .bytes()
}

/// An MP4 with an H.264 (`avc1`) video track whose samples are not decodable here: for the
/// "codec isn't supported" path.
pub fn mp4_h264(w: u32, h: u32) -> Vec<u8> {
    BmffSpec {
        brand: *b"isom",
        format: *b"avc1",
        extra: Vec::new(),
        pictures: vec![vec![0, 0, 0, 1, 0x65, 0x88]; 3],
        width: w,
        height: h,
        timescale: 1000,
        durations: vec![40, 40, 40],
        audio: true,
        per_chunk: 1,
        moov_first: true,
        large_mdat: false,
    }
    .bytes()
}

// ---- animated images -------------------------------------------------------------------------------

/// An animated GIF of solid pictures: (colour, milliseconds) each.
pub fn gif(pictures: &[([u8; 4], u32)], w: u32, h: u32) -> Vec<u8> {
    use image::codecs::gif::{GifEncoder, Repeat};
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        let _ = enc.set_repeat(Repeat::Infinite);
        let frames = pictures.iter().map(|(c, ms)| {
            image::Frame::from_parts(image::RgbaImage::from_pixel(w, h, image::Rgba(*c)), 0, 0, image::Delay::from_numer_denom_ms(*ms, 1))
        });
        let _ = enc.encode_frames(frames);
    }
    out
}

/// An animated PNG of solid pictures: (colour, milliseconds) each.
pub fn apng(pictures: &[([u8; 4], u32)], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let _ = enc.set_animated(pictures.len() as u32, 0);
        if let Ok(mut writer) = enc.write_header() {
            for (c, ms) in pictures {
                let rgba: Vec<u8> = c.iter().copied().cycle().take(w as usize * h as usize * 4).collect();
                let _ = writer.set_frame_delay(*ms as u16, 1000);
                let _ = writer.write_image_data(&rgba);
            }
            let _ = writer.finish();
        }
    }
    out
}

/// An animated WebP of solid lossless pictures: (colour, milliseconds) each.
pub fn webp_animation(pictures: &[([u8; 4], u32)], w: u32, h: u32) -> Vec<u8> {
    let mut frames = Vec::new();
    for (c, ms) in pictures {
        let rgba: Vec<u8> = c.iter().copied().cycle().take(w as usize * h as usize * 4).collect();
        let mut still = Vec::new();
        let enc = image::codecs::webp::WebPEncoder::new_lossless(Cursor::new(&mut still));
        let _ = enc.encode(&rgba, w, h, ExtendedColorType::Rgba8);
        // The bitstream chunk of the still file (after RIFF, size, WEBP).
        let mut p = 12usize;
        let mut vp8l = None;
        while p + 8 <= still.len() {
            let size = crate::bytes::le32(&still, p + 4).unwrap_or(0) as usize;
            if still.get(p..p + 4) == Some(b"VP8L") {
                vp8l = still.get(p..(p + 8 + size + (size & 1)).min(still.len())).map(<[u8]>::to_vec);
                break;
            }
            p += 8 + size + (size & 1);
        }
        let Some(vp8l) = vp8l else { continue };
        let le24 = |v: u32| [v as u8, (v >> 8) as u8, (v >> 16) as u8];
        let mut anmf = [le24(0), le24(0), le24(w - 1), le24(h - 1), le24(*ms)].concat();
        anmf.push(0);
        anmf.extend_from_slice(&vp8l);
        frames.push(riff(b"ANMF", &anmf));
    }
    let le24 = |v: u32| [v as u8, (v >> 8) as u8, (v >> 16) as u8];
    let mut vp8x = vec![0x12, 0, 0, 0];
    vp8x.extend_from_slice(&le24(w - 1));
    vp8x.extend_from_slice(&le24(h - 1));
    let mut body = b"WEBP".to_vec();
    body.extend_from_slice(&riff(b"VP8X", &vp8x));
    body.extend_from_slice(&riff(b"ANIM", &[0, 0, 0, 0, 0, 0]));
    frames.iter().for_each(|f| body.extend_from_slice(f));
    riff(b"RIFF", &body)
}

// ---- Matroska ------------------------------------------------------------------------------------------

/// An EBML element: the id's bytes, an 8-byte size, the payload.
fn ebml(id: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.push(0x01);
    v.extend_from_slice(&(payload.len() as u64).to_be_bytes()[1..]);
    v.extend_from_slice(payload);
    v
}

/// A WebM or Matroska file (by `doc_type`) with a video track of `codec_id` (`V_VP9`) and no
/// pictures: for the refusal path.
pub fn matroska(doc_type: &str, codec_id: &str) -> Vec<u8> {
    let header = ebml(&[0x1A, 0x45, 0xDF, 0xA3], &ebml(&[0x42, 0x82], doc_type.as_bytes()));
    let entry = ebml(&[0xAE], &[ebml(&[0x83], &[1]), ebml(&[0x86], codec_id.as_bytes())].concat());
    let tracks = ebml(&[0x16, 0x54, 0xAE, 0x6B], &entry);
    let segment = ebml(&[0x18, 0x53, 0x80, 0x67], &tracks);
    [header, segment].concat()
}
