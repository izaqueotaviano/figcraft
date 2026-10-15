//! Animated images: GIF, animated WebP and animated PNG (APNG), decoded by the `image` crate. Every
//! picture is decoded when the file opens (they depend on the ones before them), within
//! [`MAX_ANIMATION_FRAMES`] pictures and [`MAX_ANIMATION_BYTES`] bytes; a damaged picture ends the
//! animation there. Like browsers, a picture shown for less than 20 ms is shown for 100 ms (many
//! files say 0 and mean "the default").

use std::io::Cursor;
use std::sync::Arc;

use image::AnimationDecoder;
use image::ImageDecoder;

use crate::bytes::{be32, tag};
use crate::{Container, Frame, MAX_ANIMATION_BYTES, MAX_ANIMATION_FRAMES, Media, MediaError, MediaInfo, MediaKind, Source, check_size, corrupt};

/// Whether a PNG file is animated: an `acTL` chunk comes before its first `IDAT`.
pub(crate) fn png_is_animated(b: &[u8]) -> bool {
    let mut p = 8usize;
    // The animation control chunk is among the first few.
    for _ in 0..64 {
        let (Some(len), Some(kind)) = (be32(b, p), tag(b, p + 4)) else { return false };
        match &kind {
            b"acTL" => return true,
            b"IDAT" | b"IEND" => return false,
            _ => {}
        }
        // Length, type, data, CRC.
        let Some(next) = p.checked_add(12).and_then(|n| n.checked_add(len as usize)) else { return false };
        p = next;
    }
    false
}

/// Whether a WebP file is animated: its extended header (`VP8X`) has the animation flag.
pub(crate) fn webp_is_animated(b: &[u8]) -> bool {
    tag(b, 12) == Some(*b"VP8X") && b.get(20).is_some_and(|flags| flags & 0x02 != 0)
}

/// How long a picture shows (milliseconds) from its delay of `numer / denom` milliseconds.
fn picture_ms(numer: u32, denom: u32) -> f64 {
    let ms = if denom == 0 { 0.0 } else { f64::from(numer) / f64::from(denom) };
    if ms < 20.0 { 100.0 } else { ms }
}

pub(crate) fn open(bytes: &[u8], container: Container) -> Result<Media, MediaError> {
    let (w, h, frames, codec) = match container {
        Container::Gif => {
            let dec = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| corrupt(e.to_string()))?;
            let (w, h) = dec.dimensions();
            check_size(w, h)?;
            (w, h, collect(dec.into_frames(), w, h), "GIF")
        }
        Container::WebP => {
            let dec = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| corrupt(e.to_string()))?;
            let (w, h) = dec.dimensions();
            check_size(w, h)?;
            (w, h, collect(dec.into_frames(), w, h), "WebP")
        }
        Container::Apng => {
            let dec = image::codecs::png::PngDecoder::new(Cursor::new(bytes)).map_err(|e| corrupt(e.to_string()))?;
            let (w, h) = dec.dimensions();
            check_size(w, h)?;
            let dec = dec.apng().map_err(|e| corrupt(e.to_string()))?;
            (w, h, collect(dec.into_frames(), w, h), "APNG")
        }
        _ => return Err(MediaError::NotMedia),
    };
    let (pictures, delays) = frames;
    if pictures.len() < 2 {
        // One picture is a still image, not an animation.
        return Err(MediaError::NotMedia);
    }
    // Summed in milliseconds and divided once, so a picture that starts at 300 ms starts at 0.3 s
    // exactly as a caller writes it.
    let mut starts = Vec::with_capacity(delays.len());
    let mut ms = 0.0;
    for d in &delays {
        starts.push(ms / 1000.0);
        ms += d;
    }
    let at = ms / 1000.0;
    let info = MediaInfo {
        container,
        kind: MediaKind::Animation,
        codec: codec.into(),
        width: w,
        height: h,
        duration: at,
        frame_count: u32::try_from(pictures.len()).unwrap_or(u32::MAX),
        frame_rate: pictures.len() as f64 / at,
        has_audio: false,
    };
    Ok(Media::new(info, starts, Source::Frames(pictures)))
}

/// Every picture of `frames` with how long it shows (milliseconds), as many as the limits allow.
fn collect(frames: image::Frames<'_>, w: u32, h: u32) -> (Vec<Arc<Frame>>, Vec<f64>) {
    let picture_bytes = u64::from(w) * u64::from(h) * 4;
    let most = usize::try_from(MAX_ANIMATION_BYTES / picture_bytes.max(1)).unwrap_or(usize::MAX).min(MAX_ANIMATION_FRAMES);
    let (mut pictures, mut delays) = (Vec::new(), Vec::new());
    for f in frames.take(most) {
        let Ok(f) = f else { break };
        let (numer, denom) = f.delay().numer_denom_ms();
        let buf = f.into_buffer();
        // Pictures are whole canvases; one of another size ends the animation.
        if buf.dimensions() != (w, h) {
            break;
        }
        pictures.push(Arc::new(Frame { width: w, height: h, rgba: buf.into_raw() }));
        delays.push(picture_ms(numer, denom));
    }
    (pictures, delays)
}
