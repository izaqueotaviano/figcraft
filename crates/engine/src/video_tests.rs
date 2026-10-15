//! Video fills through commands: placing videos and animations (and refusing what can't play),
//! `video.info`, `video.set`, the media triggers and actions of interactions, the file round trip,
//! copy and paste, opening a video file, junk values.

use base64::Engine as _;
use figcraft_color::{ImageScaleMode, PaintKind, VideoPaint};
use figcraft_doc::NodeId;
use figcraft_doc::proto::{Action, MAX_MEDIA_SECONDS, MediaToggle, Trigger};
use figcraft_media::testing;
use serde_json::{Value, json};

use crate::params::id_of;
use crate::{EngineError, Session};

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// `image.place` of `bytes` at (10, 20), with `extra` params.
fn place(s: &mut Session, bytes: &[u8], extra: Value) -> Result<Value, EngineError> {
    let mut p = json!({"data": b64(bytes), "x": 10, "y": 20});
    if let (Some(o), Value::Object(x)) = (p.as_object_mut(), extra) {
        o.extend(x);
    }
    s.execute("image.place", &p)
}

fn placed(s: &mut Session, bytes: &[u8], extra: Value) -> NodeId {
    id_of(&place(s, bytes, extra).unwrap()["id"]).unwrap()
}

fn video(s: &Session, id: NodeId) -> VideoPaint {
    match &s.doc().unwrap().node(id).unwrap().fills[0].kind {
        PaintKind::Video(v) => v.clone(),
        other => panic!("not a video fill: {other:?}"),
    }
}

fn avi() -> Vec<u8> {
    testing::avi_mjpeg(10, 64, 48, 10, true, false)
}

fn frame(s: &mut Session) -> NodeId {
    id_of(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 200, "height": 200})).unwrap()["id"]).unwrap()
}

#[test]
fn a_motion_jpeg_avi_is_placed_as_a_video_layer() {
    let mut s = Session::new();
    let file = avi();
    let r = place(&mut s, &file, json!({})).unwrap();
    let id = id_of(&r["id"]).unwrap();
    assert_eq!((r["container"].as_str(), r["codec"].as_str(), r["kind"].as_str()), (Some("AVI"), Some("Motion-JPEG"), Some("video")));
    assert_eq!((r["pixelWidth"].as_u64(), r["pixelHeight"].as_u64(), r["frames"].as_u64()), (Some(64), Some(48), Some(10)));
    assert_eq!(r["hasAudio"], json!(false));
    assert!((r["duration"].as_f64().unwrap() - 1.0).abs() < 1e-9, "{r}");
    let doc = s.doc().unwrap();
    let n = doc.node(id).unwrap();
    assert_eq!((n.name.as_str(), n.width, n.height), ("Video", 64.0, 48.0));
    assert_eq!(s.selection(), vec![id]);
    let v = video(&s, id);
    assert!(v.autoplay && !v.looping && !v.muted && v.poster == 0.0 && v.scale_mode == ImageScaleMode::Fill, "{v:?}");
    let data = doc.images.get(&v.video).unwrap();
    assert_eq!((data.width, data.height), (64, 48));
    assert_eq!(data.bytes.as_slice(), file.as_slice());
    // Placing the same file again shares its asset.
    let again = placed(&mut s, &file, json!({}));
    assert_eq!(video(&s, again).video, v.video);
    assert_eq!(s.doc().unwrap().images.len(), 1);
    // One undo step takes the layer back out.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().node(again).is_none());
    assert!(s.doc().unwrap().node(id).is_some());
}

#[test]
fn movs_and_mp4s_are_videos_too_and_large_ones_are_fitted() {
    let mut s = Session::new();
    for (bytes, container) in [(testing::mov_mjpeg(6, 64, 48, 6, false), "MOV"), (testing::mp4_mjpeg(6, 64, 48, 6), "MP4")] {
        let r = place(&mut s, &bytes, json!({"name": "Clip"})).unwrap();
        assert_eq!(r["container"], container, "{r}");
        let n = s.doc().unwrap().node(id_of(&r["id"]).unwrap()).unwrap().clone();
        assert_eq!(n.name, "Clip");
        assert!(matches!(n.fills[0].kind, PaintKind::Video(_)));
    }
    // Pictures bigger than 1000 px are fitted into it when placed, like images; the file's size stays.
    let r = place(&mut s, &testing::synthetic(2000, 1000, 5, 5), json!({})).unwrap();
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(1000.0), Some(500.0)));
    assert_eq!((r["pixelWidth"].as_u64(), r["pixelHeight"].as_u64()), (Some(2000), Some(1000)));
    // A width gives the height by the picture's shape.
    let r = place(&mut s, &testing::synthetic(200, 100, 5, 5), json!({"width": 80})).unwrap();
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(80.0), Some(40.0)));
}

#[test]
fn placing_takes_the_playing_settings() {
    let mut s = Session::new();
    let id = placed(&mut s, &avi(), json!({"autoplay": false, "loop": true, "muted": "yes", "poster": 0.5, "mode": "fit"}));
    let v = video(&s, id);
    assert!(!v.autoplay && v.looping && v.muted, "{v:?}");
    assert_eq!((v.poster, v.scale_mode), (0.5, ImageScaleMode::Fit));
    // The poster stays inside the video (1 s long); junk poster values mean the first picture.
    for (poster, want) in [(json!(99), 1.0), (json!(-3), 0.0), (json!("later"), 0.0)] {
        let id = placed(&mut s, &avi(), json!({ "poster": poster }));
        assert_eq!(video(&s, id).poster, want, "{poster}");
    }
}

#[test]
fn animated_images_stay_pictures_unless_asked_to_be_videos() {
    let mut s = Session::new();
    let gif = testing::gif(&[([255, 0, 0, 255], 100), ([0, 255, 0, 255], 100), ([0, 0, 255, 255], 100)], 16, 12);
    // By default an animated GIF is the picture it always was.
    let id = placed(&mut s, &gif, json!({}));
    assert!(matches!(s.doc().unwrap().node(id).unwrap().fills[0].kind, PaintKind::Image(_)));
    assert_eq!(s.doc().unwrap().node(id).unwrap().name, "Image");
    // As a video it is one a prototype can control.
    let r = place(&mut s, &gif, json!({"video": true})).unwrap();
    let id = id_of(&r["id"]).unwrap();
    assert_eq!((r["container"].as_str(), r["kind"].as_str(), r["frames"].as_u64()), (Some("GIF"), Some("animation"), Some(3)));
    assert!((r["duration"].as_f64().unwrap() - 0.3).abs() < 1e-9, "{r}");
    assert!(matches!(s.doc().unwrap().node(id).unwrap().fills[0].kind, PaintKind::Video(_)));
    // A video file asked not to be a video is not a picture the image decoder reads.
    let err = place(&mut s, &avi(), json!({"video": false})).unwrap_err().to_string();
    assert!(err.contains("image"), "{err}");
}

#[test]
fn what_cannot_play_is_refused_by_name_and_not_kept() {
    let mut s = Session::new();
    s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
    let before = (s.doc().unwrap().node_count(), s.doc().unwrap().images.len());
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (testing::mp4_h264(64, 48), "This video codec isn't supported yet: H.264"),
        (testing::matroska("webm", "V_VP9"), "This video codec isn't supported yet: VP9"),
        (testing::avi(b"H264", &[vec![0u8; 40]], 64, 48, 10, false), "This video codec isn't supported yet: H.264"),
        (b"RIFF\0\0\0\0AVI not really".to_vec(), ""),
    ];
    for (bytes, message) in cases {
        let err = place(&mut s, &bytes, json!({})).unwrap_err();
        assert!(matches!(err, EngineError::Other(_)), "the message is the whole error: {err:?}");
        assert!(err.to_string().contains(message), "{err}");
        // No layer, no asset.
        assert_eq!((s.doc().unwrap().node_count(), s.doc().unwrap().images.len()), before, "{message}");
    }
    // Not media at all, asked to be a video.
    let err = place(&mut s, b"hello", json!({"video": true})).unwrap_err().to_string();
    assert!(err.contains("isn't a video"), "{err}");
    // Too large: refused before anything is read.
    let big = vec![0u8; figcraft_media::MAX_FILE_BYTES + 1];
    let err = place(&mut s, &big, json!({"video": true})).unwrap_err().to_string();
    assert!(err.contains("too large"), "{err}");
    assert_eq!((s.doc().unwrap().node_count(), s.doc().unwrap().images.len()), before);
}

#[test]
fn video_info_says_what_the_file_is_and_how_it_plays() {
    let mut s = Session::new();
    let id = placed(&mut s, &testing::avi_mjpeg(20, 64, 48, 10, true, true), json!({"loop": true}));
    let plain = s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
    let r = s.execute("video.info", &json!({"ids": [id, plain["id"]]})).unwrap();
    let list = r["videos"].as_array().unwrap();
    assert_eq!(list.len(), 1, "the rectangle has no video: {r}");
    let v = &list[0];
    assert_eq!((v["codec"].as_str(), v["frames"].as_u64(), v["hasAudio"].clone()), (Some("Motion-JPEG"), Some(20), json!(true)));
    assert_eq!(
        (v["loop"].clone(), v["autoplay"].clone(), v["muted"].clone(), v["mode"].as_str()),
        (json!(true), json!(true), json!(false), Some("fill"))
    );
    assert!((v["duration"].as_f64().unwrap() - 2.0).abs() < 1e-9 && v["bytes"].as_u64().unwrap() > 100, "{v}");
    // A fill whose file is not in the document says so, and so does one whose file doesn't open.
    let gone = id_of(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap()["id"]).unwrap();
    s.doc_mut().unwrap().get_mut(gone).unwrap().fills = vec![figcraft_color::Paint::video("0123", ImageScaleMode::Fill)];
    let r = s.execute("video.info", &json!({"ids": [gone]})).unwrap();
    assert!(r["videos"][0]["error"].as_str().unwrap().contains("isn't in the file"), "{r}");
    // One whose file is not a video that opens.
    let hash = figcraft_format::image_hash(b"not a video");
    s.doc_mut()
        .unwrap()
        .images
        .insert(hash.clone(), figcraft_doc::ImageData { bytes: std::sync::Arc::new(b"not a video".to_vec()), width: 0, height: 0 });
    s.doc_mut().unwrap().get_mut(gone).unwrap().fills = vec![figcraft_color::Paint::video(hash, ImageScaleMode::Fill)];
    let r = s.execute("video.info", &json!({"ids": [gone]})).unwrap();
    assert!(r["videos"][0]["error"].as_str().unwrap().contains("isn't a video"), "{r}");
}

#[test]
fn video_set_changes_how_it_plays() {
    let mut s = Session::new();
    let id = placed(&mut s, &avi(), json!({}));
    s.execute("video.set", &json!({"ids": [id], "autoplay": false, "loop": "true", "muted": 1, "mode": "FIT", "poster": 0.25})).unwrap();
    let v = video(&s, id);
    assert!(!v.autoplay && v.looping && v.muted, "{v:?}");
    assert_eq!((v.poster, v.scale_mode), (0.25, ImageScaleMode::Fit));
    // Selection is the default target; the poster stays within the video.
    let r = s.execute("video.set", &json!({"poster": 50})).unwrap();
    assert_eq!(r["changed"], 1);
    assert_eq!(video(&s, id).poster, 1.0);
    // Undo restores the one step.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(video(&s, id).poster, 0.25);
    // Errors, and nothing changed.
    let before = video(&s, id);
    for junk in [
        json!({}),
        json!({"poster": "soon"}),
        json!({"poster": -1}),
        json!({"mode": "stretch"}),
        json!({"mode": 4}),
        json!({"autoplay": "perhaps"}),
        json!({"loop": [true]}),
        json!({"muted": {"x": 1}, "index": 5}),
        json!({"muted": true, "index": -1}),
        json!({"muted": true, "index": 1.5}),
    ] {
        assert!(s.execute("video.set", &junk).is_err(), "{junk}");
        assert_eq!(video(&s, id), before, "{junk}");
    }
    // A layer without a video says so.
    let plain = s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
    let err = s.execute("video.set", &json!({"ids": [plain["id"]], "muted": true})).unwrap_err().to_string();
    assert!(err.contains("no video fills"), "{err}");
}

#[test]
fn a_saved_file_keeps_its_video_and_its_size() {
    let mut s = Session::new();
    let file = testing::mov_mjpeg(8, 64, 48, 8, false);
    let id = placed(&mut s, &file, json!({"loop": true, "poster": 0.5}));
    let bytes = figcraft_format::save_bytes(s.doc().unwrap(), None).unwrap();
    let (back, issues) = figcraft_format::load_bytes(&bytes).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    let PaintKind::Video(v) = &back.node(id).unwrap().fills[0].kind else { panic!("the video fill is gone") };
    assert!(v.looping && v.autoplay && v.poster == 0.5);
    let data = back.images.get(&v.video).unwrap();
    assert_eq!(data.bytes.as_slice(), file.as_slice());
    // A file without the size listed (loaded from its bytes) gets it from the video.
    assert_eq!((data.width, data.height), (64, 48));
}

#[test]
fn copy_and_paste_carry_the_video_to_another_file() {
    let mut s = Session::new();
    let file = avi();
    let id = placed(&mut s, &file, json!({}));
    let hash = video(&s, id).video;
    s.execute("edit.copy", &json!({"ids": [id]})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
    assert!(s.doc().unwrap().images.is_empty());
    s.execute("edit.paste", &json!({})).unwrap();
    let pasted = s.selection()[0];
    assert_eq!(video(&s, pasted).video, hash);
    assert_eq!(s.doc().unwrap().images.get(&hash).unwrap().bytes.as_slice(), file.as_slice());
}

#[test]
fn a_video_file_imports_and_opens_from_a_path() {
    let dir = std::env::temp_dir().join(format!("figcraft-video-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Intro clip.avi");
    std::fs::write(&path, avi()).unwrap();
    let mut s = Session::new();
    let r = s.execute("file.import", &json!({"path": path.to_string_lossy(), "x": 5, "y": 6})).unwrap();
    let id = id_of(&r["id"]).unwrap();
    let n = s.doc().unwrap().node(id).unwrap();
    assert_eq!((n.name.as_str(), n.width, n.height), ("Intro clip", 64.0, 48.0));
    assert!(matches!(n.fills[0].kind, PaintKind::Video(_)));
    // Opened as a file of its own: one page with the video.
    s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
    let doc = s.doc().unwrap();
    let layer = doc.nodes().find(|n| n.name == "Video").unwrap();
    assert!(matches!(layer.fills[0].kind, PaintKind::Video(_)));
    assert_eq!(doc.images.len(), 1);
    // A video that can't play isn't opened.
    let bad = dir.join("bad.mp4");
    std::fs::write(&bad, testing::mp4_h264(64, 48)).unwrap();
    let err = s.execute("file.open", &json!({"path": bad.to_string_lossy()})).unwrap_err().to_string();
    assert!(err.contains("isn't supported yet: H.264"), "{err}");
    let err = s.execute("file.import", &json!({"path": bad.to_string_lossy()})).unwrap_err().to_string();
    assert!(err.contains("isn't supported yet: H.264"), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn media_triggers_and_actions_are_stored_clamped_and_checked() {
    let mut s = Session::new();
    let screen = frame(&mut s);
    let clip = placed(&mut s, &avi(), json!({"parent": screen}));
    let button = id_of(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10, "parent": screen})).unwrap()["id"]).unwrap();
    let add = |s: &mut Session, id: NodeId, trigger: Value, action: Value| {
        s.execute("proto.interaction.add", &json!({"id": id, "trigger": trigger, "action": action}))
    };
    // A hit time, inside the day.
    add(&mut s, clip, json!({"type": "mediaHit", "time": 1e300}), json!({"type": "back"})).unwrap();
    add(&mut s, clip, json!({"type": "mediaHit", "time": -2}), json!({"type": "back"})).unwrap();
    add(&mut s, clip, json!({"type": "mediaHit", "time": 2.5}), json!({"type": "back"})).unwrap();
    add(&mut s, clip, json!({"type": "mediaEnd"}), json!({"type": "back"})).unwrap();
    let times: Vec<Trigger> = s.doc().unwrap().node(clip).unwrap().interactions.iter().map(|i| i.trigger.clone()).collect();
    assert_eq!(
        times,
        [Trigger::MediaHit { time: MAX_MEDIA_SECONDS }, Trigger::MediaHit { time: 0.0 }, Trigger::MediaHit { time: 2.5 }, Trigger::MediaEnd]
    );
    // The four actions, with their targets and defaults.
    add(&mut s, button, json!({"type": "onClick"}), json!({"type": "mediaPlayPause", "target": clip})).unwrap();
    add(&mut s, button, json!({"type": "onClick"}), json!({"type": "mediaMuteUnmute", "mode": "on"})).unwrap();
    add(&mut s, button, json!({"type": "onClick"}), json!({"type": "mediaSkip", "target": clip})).unwrap();
    add(&mut s, button, json!({"type": "onClick"}), json!({"type": "mediaSkip", "target": clip, "seconds": -1e300})).unwrap();
    add(&mut s, button, json!({"type": "onClick"}), json!({"type": "mediaSkipTo", "target": clip, "time": 1e300})).unwrap();
    let actions: Vec<Action> = s.doc().unwrap().node(button).unwrap().interactions.iter().map(|i| i.actions[0].clone()).collect();
    assert_eq!(
        actions,
        [
            Action::MediaPlayPause { target: Some(clip), mode: MediaToggle::Toggle },
            Action::MediaMuteUnmute { target: None, mode: MediaToggle::On },
            Action::MediaSkip { target: Some(clip), seconds: figcraft_doc::proto::DEFAULT_SKIP },
            Action::MediaSkip { target: Some(clip), seconds: -MAX_MEDIA_SECONDS },
            Action::MediaSkipTo { target: Some(clip), time: MAX_MEDIA_SECONDS },
        ]
    );
    // A target that isn't a layer is refused; patching a time clamps too.
    let err = add(&mut s, button, json!({"type": "onClick"}), json!({"type": "mediaPlayPause", "target": 987_654})).unwrap_err().to_string();
    assert!(err.contains("media target"), "{err}");
    s.execute("proto.interaction.set", &json!({"id": clip, "index": 2, "trigger": {"type": "mediaHit", "time": 7}})).unwrap();
    assert_eq!(s.doc().unwrap().node(clip).unwrap().interactions[2].trigger, Trigger::MediaHit { time: 7.0 });
    // They are saved in the file, and copying the screen points the controls at the copy.
    let bytes = figcraft_format::save_bytes(s.doc().unwrap(), None).unwrap();
    let (back, issues) = figcraft_format::load_bytes(&bytes).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(back.node(button).unwrap().interactions, s.doc().unwrap().node(button).unwrap().interactions);
    let copy = id_of(&s.execute("edit.duplicate", &json!({"ids": [screen]})).unwrap()["ids"][0]).unwrap();
    let doc = s.doc().unwrap();
    let kids: Vec<NodeId> = doc.node(copy).unwrap().children.clone();
    let new_clip =
        *kids.iter().find(|k| doc.node(**k).is_some_and(|n| matches!(n.fills.first().map(|f| &f.kind), Some(PaintKind::Video(_))))).unwrap();
    let new_button = *kids.iter().find(|k| **k != new_clip).unwrap();
    assert_ne!(new_clip, clip);
    assert_eq!(doc.node(new_button).unwrap().interactions[0].actions[0].media_target(), Some(new_clip));
}

#[test]
fn junk_video_params_never_panic() {
    let mut s = Session::new();
    let screen = frame(&mut s);
    let clip = placed(&mut s, &avi(), json!({"parent": screen}));
    let junk = [
        json!({}),
        json!(null),
        json!([1, 2]),
        json!({"ids": ["x", -1, 99999]}),
        json!({"id": clip, "autoplay": [1], "loop": {"x": 1}, "muted": "maybe", "poster": 1e308, "mode": 5, "index": -1}),
        json!({"id": clip, "autoplay": null, "poster": "NaN", "mode": null, "index": 1e308}),
        json!({"id": clip, "poster": -1e308, "mode": "fit", "index": 0}),
        json!({"id": clip, "trigger": {"type": "mediaHit", "time": "x"}, "action": {"type": "mediaSkip", "seconds": "far", "target": "nope"}}),
        json!({"id": clip, "trigger": {"type": "mediaHit", "time": [1]}, "action": {"type": "mediaSkipTo", "time": {"x": 1}, "target": -5}}),
        json!({"id": clip, "trigger": {"type": "mediaEnd", "time": 5}, "action": {"type": "mediaPlayPause", "mode": "sideways"}}),
        json!({"id": clip, "index": 0, "action": {"type": "mediaMuteUnmute", "target": 1e300, "mode": null}}),
        json!({"video": true, "data": 5, "path": [1], "mode": "fit", "poster": "x"}),
    ];
    for cmd in ["video.set", "video.info", "proto.interaction.add", "proto.interaction.set", "proto.action.add", "proto.action.set", "fill.set"] {
        for j in &junk {
            if let Err(EngineError::Internal { cmd, msg }) = s.execute(cmd, j) {
                panic!("{cmd} panicked on {j}: {msg}");
            }
        }
    }
    // Junk bytes placed as videos or pictures: errors, never panics.
    for bytes in [&b""[..], b"RIFF", b"RIFF\x04\0\0\0AVI ", b"\0\0\0\x20ftypmp42", b"\x1A\x45\xDF\xA3", b"GIF89a", b"FCSYNTH1", &[0xFF; 64][..]] {
        for video in [json!(true), json!(false), json!(null)] {
            if let Err(EngineError::Internal { cmd, msg }) = place(&mut s, bytes, json!({"video": video})) {
                panic!("{cmd} panicked on {bytes:?}: {msg}");
            }
        }
    }
}
