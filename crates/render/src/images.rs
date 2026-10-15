//! Decoded image cache (content hash + filters + colour profile → premultiplied pixmap). Images
//! are converted from their own colour profile (see [`crate::image_profile`]; sRGB when they have
//! none) into the document's space, once per image and profile.
//!
//! Animated GIFs keep their original bytes like any image; their first frame is the image
//! everywhere (canvas, exports). The presenter plays them: [`frame_at`] decodes every frame on
//! first use (capped by [`MAX_FRAMES`] and [`MAX_ANIMATION_BYTES`]) and picks the one due.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use figcraft_color::{ColorProfile, ImageFilters, PaintKind};
use figcraft_doc::{Document, ImageData, NodeId};
use tiny_skia::Pixmap;

type Key = (String, [i32; 7], ColorProfile);

fn cache() -> &'static Mutex<HashMap<Key, Arc<Pixmap>>> {
    static C: OnceLock<Mutex<HashMap<Key, Arc<Pixmap>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fkey(f: &ImageFilters) -> [i32; 7] {
    let q = |v: f32| (v * 1000.0).round() as i32;
    [q(f.exposure), q(f.contrast), q(f.saturation), q(f.temperature), q(f.tint), q(f.highlights), q(f.shadows)]
}

/// Decode encoded image bytes into a premultiplied sRGB pixmap (max 16k per side).
pub fn decode(bytes: &[u8]) -> Option<Pixmap> {
    decode_in(bytes, ColorProfile::Srgb).map(|(p, _)| p)
}

/// Decode encoded image bytes into a premultiplied pixmap (max 16k per side): converted from the
/// image's own profile into `target` (`false`), or as sRGB when it has none (`true`: the sRGB →
/// `target` conversion is still to do).
fn decode_in(bytes: &[u8], target: ColorProfile) -> Option<(Pixmap, bool)> {
    let mut dec = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_decoder().ok()?;
    let source = crate::image_profile::Source::read(bytes, &mut dec);
    let img = image::DynamicImage::from_decoder(dec).ok()?;
    match source.convert(&img, target) {
        Some(rgba) => Some((premultiplied(&rgba)?, false)),
        None => Some((premultiplied(&img.to_rgba8())?, true)),
    }
}

fn profile_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The name of image `hash`'s own colour profile (empty for an unnamed one); `None` when unknown,
/// and it's taken as sRGB. Cached.
pub fn profile_name(hash: &str, data: Option<&ImageData>) -> Option<String> {
    if let Some(n) = profile_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(hash) {
        return n.clone();
    }
    let name = crate::image_profile::Source::of(&data?.bytes).name;
    let mut c = profile_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if c.len() > 256 {
        c.clear();
    }
    c.insert(hash.to_string(), name.clone());
    name
}

/// Straight RGBA → a premultiplied pixmap (max 16k per side).
fn premultiplied(rgba: &image::RgbaImage) -> Option<Pixmap> {
    let (w, h) = rgba.dimensions();
    premultiplied_raw(w, h, rgba.as_raw())
}

/// Straight RGBA bytes (`w × h × 4`) → a premultiplied pixmap (max 16k per side).
fn premultiplied_raw(w: u32, h: u32, rgba: &[u8]) -> Option<Pixmap> {
    if w == 0 || h == 0 || w > 16_384 || h > 16_384 {
        return None;
    }
    let mut p = Pixmap::new(w, h)?;
    for (dst, src) in p.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(rgba.chunks_exact(4)) {
        let a = u32::from(src.get(3).copied().unwrap_or(255));
        let pm = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
        dst.copy_from_slice(&[
            pm(src.first().copied().unwrap_or(0)),
            pm(src.get(1).copied().unwrap_or(0)),
            pm(src.get(2).copied().unwrap_or(0)),
            a as u8,
        ]);
    }
    Some(p)
}

/// The decoded (and filtered) image for `hash` in the colour space `profile`, cached.
pub fn get(hash: &str, data: Option<&ImageData>, filters: &ImageFilters, profile: ColorProfile) -> Option<Arc<Pixmap>> {
    let key = (hash.to_string(), fkey(filters), profile);
    if let Some(p) = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
        return Some(p.clone());
    }
    let (mut p, srgb) = decode_in(&data?.bytes, profile)?;
    // Images with their own profile are in `profile` already (sRGB → sRGB is the identity).
    finish(&mut p, filters, if srgb { profile } else { ColorProfile::Srgb });
    let p = Arc::new(p);
    let mut c = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if c.len() > 256 {
        c.clear();
    }
    c.insert(key, p.clone());
    Some(p)
}

/// The filters, then the conversion into the document's colour space.
fn finish(p: &mut Pixmap, filters: &ImageFilters, profile: ColorProfile) {
    if !filters.is_identity() {
        apply_filters(p, filters);
    }
    ColorProfile::Srgb.conversion_to(profile).apply_pixels(p.data_mut());
}

/// The most decoded video pictures kept (each is a screenful or more of pixels).
const MAX_VIDEO_PICTURES: usize = 8;
/// The most bytes those pictures take together.
const MAX_VIDEO_PICTURE_BYTES: usize = 96 << 20;

type VideoKey = (String, u32, ColorProfile);
/// The kept pictures, least recently used first.
type VideoPictures = Vec<(VideoKey, Arc<Pixmap>)>;

fn video_cache() -> &'static Mutex<VideoPictures> {
    static C: OnceLock<Mutex<VideoPictures>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Vec::new()))
}

/// The picture video (or animated image) `hash` shows `t` seconds in, as a premultiplied pixmap in
/// the document's colour space. The file is opened once and shared ([`figcraft_media::shared`]),
/// the last few pictures are kept; `None` when the file isn't in the document, doesn't open or
/// has no picture that decodes. Pictures are taken as sRGB.
pub fn video_frame(hash: &str, data: Option<&ImageData>, t: f64, profile: ColorProfile) -> Option<Arc<Pixmap>> {
    let media = figcraft_media::shared(hash, &data?.bytes).ok()?;
    let index = media.frame_index_at(t);
    let key = (hash.to_string(), index, profile);
    {
        let mut c = video_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(at) = c.iter().position(|(k, _)| *k == key) {
            // Most recently used last.
            let entry = c.remove(at);
            let pm = entry.1.clone();
            c.push(entry);
            return Some(pm);
        }
    }
    let frame = media.frame_at(t).ok()?;
    let mut pm = premultiplied_raw(frame.width, frame.height, &frame.rgba)?;
    ColorProfile::Srgb.conversion_to(profile).apply_pixels(pm.data_mut());
    let pm = Arc::new(pm);
    let mut c = video_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    c.push((key, pm.clone()));
    let mut bytes: usize = c.iter().map(|(_, p)| p.data().len()).sum();
    while c.len() > 1 && (c.len() > MAX_VIDEO_PICTURES || bytes > MAX_VIDEO_PICTURE_BYTES) {
        let (_, gone) = c.remove(0);
        bytes = bytes.saturating_sub(gone.data().len());
    }
    Some(pm)
}

/// The most frames of an animated GIF that play.
pub const MAX_FRAMES: usize = 500;
/// The most decoded bytes an animated GIF's frames take (frames past it don't play).
pub const MAX_ANIMATION_BYTES: u64 = 128 << 20;
/// The most decoded bytes the cached animations take together.
const MAX_CACHED_ANIMATION_BYTES: u64 = 256 << 20;

/// An animated GIF's frames (whole, composited), filtered and in one colour space.
pub struct Animation {
    frames: Vec<Arc<Pixmap>>,
    /// When each frame ends, in seconds from the start of a loop.
    ends: Vec<f64>,
}

impl Animation {
    /// The frame showing `t` seconds in (the animation loops) and the seconds until the next one.
    pub fn at(&self, t: f64) -> (usize, f64) {
        let total = self.ends.last().copied().unwrap_or(0.0);
        if !t.is_finite() || t <= 0.0 || total <= 0.0 {
            return (0, self.ends.first().copied().unwrap_or(f64::INFINITY));
        }
        let t = t.rem_euclid(total);
        let i = self.ends.partition_point(|e| *e <= t).min(self.ends.len().saturating_sub(1));
        (i, self.ends.get(i).map_or(f64::INFINITY, |e| e - t))
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    fn bytes(&self) -> u64 {
        self.frames.iter().map(|f| f.data().len() as u64).sum()
    }
}

/// A GIF frame's delay in seconds. Delays under 20 ms play as 100 ms, as browsers do (many GIFs
/// say 0 and mean "the default").
pub fn frame_secs(numer: u32, denom: u32) -> f64 {
    let ms = if denom == 0 { 0.0 } else { f64::from(numer) / f64::from(denom) };
    if ms < 20.0 { 0.1 } else { ms / 1000.0 }
}

/// Every frame of an animated GIF with its delay; `None` for other images, stills and GIFs too
/// big for one frame within [`MAX_ANIMATION_BYTES`]. A damaged frame ends the animation there.
/// shortcut: GIFs always loop (their repeat count is ignored); read it if a one-shot GIF matters.
pub fn decode_frames(bytes: &[u8]) -> Option<Vec<(Pixmap, f64)>> {
    use image::{AnimationDecoder, ImageDecoder};
    if image::guess_format(bytes).ok()? != image::ImageFormat::Gif {
        return None;
    }
    let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let (w, h) = dec.dimensions();
    let frame_bytes = u64::from(w) * u64::from(h) * 4;
    if frame_bytes == 0 || w > 16_384 || h > 16_384 {
        return None;
    }
    let most = (MAX_ANIMATION_BYTES / frame_bytes).min(MAX_FRAMES as u64) as usize;
    let mut out = Vec::new();
    for f in dec.into_frames().take(most) {
        let Ok(f) = f else { break };
        let (n, d) = f.delay().numer_denom_ms();
        out.push((premultiplied(f.buffer())?, frame_secs(n, d)));
    }
    (out.len() > 1).then_some(out)
}

type AnimKey = (String, [i32; 7], ColorProfile);
type AnimCache = HashMap<AnimKey, Option<Arc<Animation>>>;

fn anim_cache() -> &'static Mutex<AnimCache> {
    static C: OnceLock<Mutex<AnimCache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The animated GIF `hash`'s frames for `filters` and `profile`, decoded on first use and cached;
/// `None` for anything that doesn't move.
pub fn animation(hash: &str, data: Option<&ImageData>, filters: &ImageFilters, profile: ColorProfile) -> Option<Arc<Animation>> {
    let key = (hash.to_string(), fkey(filters), profile);
    if let Some(a) = anim_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
        return a.clone();
    }
    let a = decode_frames(&data?.bytes).map(|frames| {
        let mut end = 0.0;
        let (frames, ends) = frames
            .into_iter()
            .map(|(mut p, secs)| {
                finish(&mut p, filters, profile);
                end += secs;
                (Arc::new(p), end)
            })
            .unzip();
        Arc::new(Animation { frames, ends })
    });
    let mut c = anim_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let held: u64 = c.values().flatten().map(|a| a.bytes()).sum();
    if held + a.as_ref().map_or(0, |a| a.bytes()) > MAX_CACHED_ANIMATION_BYTES || c.len() > 256 {
        c.clear();
    }
    c.insert(key, a.clone());
    a
}

/// The frame an animated GIF shows `t` seconds in, when that isn't its first frame (the still
/// image every other path draws).
pub fn frame_at(hash: &str, data: Option<&ImageData>, filters: &ImageFilters, profile: ColorProfile, t: f64) -> Option<Arc<Pixmap>> {
    if t.is_nan() || t <= 0.0 {
        return None;
    }
    let a = animation(hash, data, filters, profile)?;
    match a.at(t).0 {
        0 => None,
        i => a.frames.get(i).cloned(),
    }
}

/// Where the animated GIFs in `root`'s visible fills and strokes are `t` seconds in: a number for
/// the frames they show (a cache key) and the seconds until one of them changes. `None` when
/// nothing there moves.
/// shortcut: text runs with their own image fills aren't looked at; add them if anyone animates text.
pub fn animation_clock(doc: &Document, root: NodeId, t: f64) -> Option<(u64, f64)> {
    let mut clock: Option<(u64, f64)> = None;
    for n in doc.subtree(root).into_iter().filter_map(|id| doc.node(id)).filter(|n| n.visible) {
        for p in n.fills.iter().chain(&n.strokes).filter(|p| p.visible) {
            if let PaintKind::Image(img) = &p.kind
                && let Some(a) = animation(&img.image, doc.images.get(&img.image), &img.filters, doc.color_profile)
            {
                let (i, next) = a.at(t);
                let (key, soonest) = clock.unwrap_or((0, f64::INFINITY));
                clock = Some((key.wrapping_mul(31).wrapping_add(i as u64 + 1), soonest.min(next)));
            }
        }
    }
    clock
}

/// Image adjustments (approximations of exposure, contrast, saturation, temperature, tint,
/// highlights and shadows) on a premultiplied pixmap.
pub fn apply_filters(p: &mut Pixmap, f: &ImageFilters) {
    let exposure = 2f32.powf(f.exposure.clamp(-1.0, 1.0) * 2.0);
    let contrast = 1.0 + f.contrast.clamp(-1.0, 1.0);
    let sat = 1.0 + f.saturation.clamp(-1.0, 1.0);
    let temp = f.temperature.clamp(-1.0, 1.0) * 0.15;
    let tint = f.tint.clamp(-1.0, 1.0) * 0.15;
    let hi = f.highlights.clamp(-1.0, 1.0) * 0.3;
    let sh = f.shadows.clamp(-1.0, 1.0) * 0.3;
    for px in p.data_mut().as_chunks_mut::<4>().0 {
        let a = f32::from(px.get(3).copied().unwrap_or(0)) / 255.0;
        if a <= 0.0 {
            continue;
        }
        let un = |i: usize| f32::from(px.get(i).copied().unwrap_or(0)) / 255.0 / a;
        let (mut r, mut g, mut b) = (un(0), un(1), un(2));
        r *= exposure;
        g *= exposure;
        b *= exposure;
        r = (r - 0.5) * contrast + 0.5;
        g = (g - 0.5) * contrast + 0.5;
        b = (b - 0.5) * contrast + 0.5;
        let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        r = l + (r - l) * sat;
        g = l + (g - l) * sat;
        b = l + (b - l) * sat;
        r += temp;
        b -= temp;
        g += tint;
        let l = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);
        let adj = hi * l * l + sh * (1.0 - l) * (1.0 - l);
        r += adj;
        g += adj;
        b += adj;
        let q = |c: f32| (c.clamp(0.0, 1.0) * a * 255.0).round() as u8;
        let (qr, qg, qb) = (q(r), q(g), q(b));
        if let Some(s) = px.get_mut(0..3) {
            s.copy_from_slice(&[qr, qg, qb]);
        }
    }
}

/// Read an image's pixel size without keeping the decode.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    reader.into_dimensions().ok()
}

/// `pm` with a transparent 1-pixel border.
pub fn pad(pm: &Pixmap) -> Option<Pixmap> {
    let mut out = Pixmap::new(pm.width() + 2, pm.height() + 2)?;
    out.draw_pixmap(1, 1, pm.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
    Some(out)
}

fn padded_cache() -> &'static Mutex<HashMap<Key, Arc<Pixmap>>> {
    static C: OnceLock<Mutex<HashMap<Key, Arc<Pixmap>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `pm` with a transparent 1-pixel border (cached).
pub fn padded(hash: &str, filters: &ImageFilters, profile: ColorProfile, pm: &Pixmap) -> Option<Arc<Pixmap>> {
    let key = (hash.to_string(), fkey(filters), profile);
    if let Some(p) = padded_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
        return Some(p.clone());
    }
    let out = Arc::new(pad(pm)?);
    let mut c = padded_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if c.len() > 256 {
        c.clear();
    }
    c.insert(key, out.clone());
    Some(out)
}
