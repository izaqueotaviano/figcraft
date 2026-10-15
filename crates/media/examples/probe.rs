//! Open a video or animated image the way the editor does and say what it is.
//!
//! ```text
//! cargo run -p figcraft-media --example probe -- FILE [SECONDS [OUT.png]]
//! cargo run -p figcraft-media --example probe --features testing -- --samples DIR
//! ```
//!
//! The first prints the file's description, and the picture showing at SECONDS (written as a PNG
//! with the third argument). The second (with the `testing` feature) writes small synthetic
//! Motion-JPEG videos and animated images into DIR to try the editor and the presenter with.

use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(feature = "testing")]
    if let [flag, dir] = args.as_slice()
        && flag == "--samples"
    {
        return samples(dir);
    }
    let Some(path) = args.first() else {
        eprintln!("usage: probe FILE [SECONDS [OUT.png]]   |   probe --samples DIR (feature `testing`)");
        return;
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return eprintln!("{path}: {e}"),
    };
    let size = bytes.len();
    match figcraft_media::Media::open(Arc::new(bytes)) {
        Ok(m) => {
            let i = m.info();
            println!("{path}: {size} bytes");
            println!(
                "{} / {} ({:?}): {}x{}, {} pictures, {:.3} s, {:.2} per second, sound: {}",
                i.container.label(),
                i.codec,
                i.kind,
                i.width,
                i.height,
                i.frame_count,
                i.duration,
                i.frame_rate,
                i.has_audio
            );
            if let Some(t) = args.get(1).and_then(|t| t.parse::<f64>().ok()) {
                let started = std::time::Instant::now();
                match m.frame_at(t) {
                    Ok(f) => {
                        println!("picture {} at {t} s: {}x{}, decoded in {:?}", m.frame_index_at(t), f.width, f.height, started.elapsed());
                        if let (Some(out), Some(img)) = (args.get(2), image::RgbaImage::from_raw(f.width, f.height, f.rgba.clone())) {
                            match img.save(out) {
                                Ok(()) => println!("wrote {out}"),
                                Err(e) => eprintln!("{out}: {e}"),
                            }
                        }
                    }
                    Err(e) => eprintln!("picture at {t} s: {e}"),
                }
            }
        }
        Err(e) => eprintln!("{path}: {e}"),
    }
}

#[cfg(feature = "testing")]
fn samples(dir: &str) {
    use figcraft_media::testing;
    let write = |name: &str, bytes: Vec<u8>| match std::fs::write(std::path::Path::new(dir).join(name), &bytes) {
        Ok(()) => println!("{name}: {} bytes", bytes.len()),
        Err(e) => eprintln!("{name}: {e}"),
    };
    let _ = std::fs::create_dir_all(dir);
    write("clip.avi", testing::avi_mjpeg(60, 320, 180, 15, false, true));
    write("clip.mov", testing::mov_mjpeg(60, 320, 180, 15, true));
    write("clip-mp4.mp4", testing::mp4_mjpeg(60, 320, 180, 15));
    write("h264.mp4", testing::mp4_h264(320, 180));
    let colours = [([220, 60, 60, 255], 300), ([60, 200, 90, 255], 300), ([60, 90, 220, 255], 300)];
    write("anim.gif", testing::gif(&colours, 96, 96));
    write("anim.png", testing::apng(&colours, 96, 96));
}
