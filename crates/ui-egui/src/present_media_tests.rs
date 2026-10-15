//! Video in the player: it plays by the player's time (a frame of the harness is 1/60 s), starts
//! over with its screen, ends and hits its times once per crossing, takes media actions on the
//! layer they name, asks for frames only while it plays, and redraws only its own area. The
//! source is synthetic: 30 pictures at 10 per second (3 s), or a few more or fewer.

use base64::Engine as _;
use egui::{Event, Key, Modifiers, Pos2, Rect, pos2};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use figcraft_media::testing;
use serde_json::{Value, json};

use crate::present_tests::{Ui as Harness, id};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn frame(s: &mut Session, name: &str, x: f64) -> NodeId {
    id(&ex(s, "frame.create", json!({"x": x, "y": 0, "width": 400, "height": 300, "name": name})))
}

fn rect(s: &mut Session, parent: NodeId, name: &str, [x, y, w, h]: [f64; 4]) -> NodeId {
    id(&ex(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h, "parent": parent, "name": name})))
}

/// The video layer `name` in `parent`: `frames` pictures at 10 per second, 160 wide (120 high).
fn clip(s: &mut Session, parent: NodeId, name: &str, frames: u32, extra: Value) -> NodeId {
    let bytes = testing::synthetic(64, 48, frames, 10);
    let mut p = json!({
        "data": base64::engine::general_purpose::STANDARD.encode(bytes),
        "video": true, "parent": parent, "name": name, "x": 20, "y": 20, "width": 160
    });
    if let (Some(o), Value::Object(x)) = (p.as_object_mut(), extra) {
        o.extend(x);
    }
    id(&ex(s, "image.place", p))
}

struct Scene {
    s: Session,
    a: NodeId,
    b: NodeId,
    clip: NodeId,
}

/// Screens A (a backdrop, the video "Clip" of `frames` pictures, with `extra` placing params) and B.
fn scene(frames: u32, extra: Value) -> Scene {
    let mut s = Session::new();
    let a = frame(&mut s, "A", 0.0);
    let b = frame(&mut s, "B", 500.0);
    rect(&mut s, a, "Back", [0.0, 0.0, 400.0, 300.0]);
    let clip = clip(&mut s, a, "Clip", frames, extra);
    Scene { s, a, b, clip }
}

/// A 60 × 30 layer in A at `x` with one interaction on click.
fn button(s: &mut Session, a: NodeId, name: &str, x: f64, actions: Value) -> NodeId {
    let b = rect(s, a, name, [x, 200.0, 60.0, 30.0]);
    ex(s, "proto.interaction.add", json!({"id": b, "trigger": {"type": "onClick"}, "actions": actions}));
    b
}

fn start(s: Session, at: NodeId) -> Harness {
    let mut u = Harness::new(s);
    u.run("proto.present", json!({"start": at}));
    u.frames(2, Vec::new);
    u
}

fn presenter(u: &Harness) -> Value {
    u.app.inspect()["presenter"].clone()
}

fn screen(u: &Harness) -> String {
    presenter(u)["name"].as_str().unwrap_or_default().to_string()
}

/// What the player says of video layer `id`.
fn video(u: &Harness, id: NodeId) -> Value {
    let all = presenter(u)["media"]["videos"].as_array().cloned().unwrap_or_default();
    all.into_iter().find(|v| v["id"] == json!(id)).unwrap_or(Value::Null)
}

fn pos(u: &Harness, id: NodeId) -> f64 {
    video(u, id)["position"].as_f64().unwrap_or(-1.0)
}

fn count(u: &Harness, key: &str) -> u64 {
    presenter(u)["media"][key].as_u64().unwrap_or(0)
}

/// The last frame asked for no frame soon (the long timers the app keeps, like its autosave, aside).
fn asleep(u: &Harness) -> bool {
    u.repaint.is_none_or(|d| d > std::time::Duration::from_secs(60))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.08
}

fn hotspot(u: &Harness, layer: NodeId) -> Rect {
    let hot = presenter(u)["hotspots"].as_array().cloned().unwrap_or_default();
    let h = hot.iter().find(|h| h["id"] == json!(layer)).unwrap_or_else(|| panic!("no hotspot on {layer}: {hot:?}"));
    let r: Vec<f32> = h["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
    Rect::from_min_max(pos2(r[0], r[1]), pos2(r[2], r[3]))
}

fn click(u: &mut Harness, layer: NodeId) {
    let at = hotspot(u, layer).center();
    u.button(at, true);
    u.button(at, false);
    u.frames(2, Vec::new);
}

fn restart(u: &mut Harness, at: Pos2) {
    let k = |pressed| Event::Key { key: Key::R, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    u.frame(vec![Event::PointerMoved(at), k(true), k(false)]);
}

fn overlays(u: &Harness) -> usize {
    presenter(u)["overlays"].as_array().map_or(0, Vec::len)
}

#[test]
fn a_video_plays_by_the_players_time_and_ends_once() {
    let Scene { s, a, clip, .. } = scene(30, json!({}));
    let mut u = start(s, a);
    let v = video(&u, clip);
    assert_eq!((v["playing"].clone(), v["frames"].clone(), v["duration"].as_f64()), (json!(true), json!(30), Some(3.0)), "{v}");
    assert!(pos(&u, clip) < 0.1);
    // One second of frames is one second of video, and ten pictures.
    u.frames(60, Vec::new);
    assert!(near(pos(&u, clip), 1.0), "{}", pos(&u, clip));
    assert!((9..=11).contains(&video(&u, clip)["frame"].as_u64().unwrap()), "{}", video(&u, clip));
    // To the end, where it stops on the last picture and says so once.
    u.frames(60 * 2 + 30, Vec::new);
    let v = video(&u, clip);
    assert_eq!(
        (v["ended"].clone(), v["playing"].clone(), v["position"].as_f64(), v["frame"].clone()),
        (json!(true), json!(false), Some(3.0), json!(29))
    );
}

#[test]
fn frames_are_asked_for_only_while_a_video_plays() {
    let Scene { s, a, clip, .. } = scene(30, json!({}));
    let mut u = start(s, a);
    // Playing: the next picture is due within a tenth of a second.
    u.frames(10, Vec::new);
    assert!(u.repaint.is_some_and(|d| d < std::time::Duration::from_millis(200)), "{:?}", u.repaint);
    // Ended: nothing asks for another frame.
    u.frames(60 * 3 + 30, Vec::new);
    assert_eq!(video(&u, clip)["ended"], json!(true));
    u.frames(5, Vec::new);
    assert!(asleep(&u), "an idle window draws nothing: {:?}", u.repaint);
    // A video that waits to be played doesn't keep the window awake either.
    let Scene { s, a, clip, .. } = scene(30, json!({"autoplay": false}));
    let mut u = start(s, a);
    u.frames(30, Vec::new);
    assert_eq!((video(&u, clip)["playing"].clone(), pos(&u, clip)), (json!(false), 0.0));
    assert!(asleep(&u), "{:?}", u.repaint);
}

#[test]
fn play_pause_toggles_and_a_video_without_autoplay_waits() {
    let Scene { mut s, a, clip, .. } = scene(30, json!({"autoplay": false}));
    let toggle = button(&mut s, a, "Toggle", 20.0, json!([{"type": "mediaPlayPause", "target": clip}]));
    let play = button(&mut s, a, "Play", 100.0, json!([{"type": "mediaPlayPause", "target": clip, "mode": "on"}]));
    let pause = button(&mut s, a, "Pause", 180.0, json!([{"type": "mediaPlayPause", "target": clip, "mode": "off"}]));
    let mut u = start(s, a);
    u.frames(30, Vec::new);
    assert_eq!((video(&u, clip)["playing"].clone(), pos(&u, clip)), (json!(false), 0.0), "waits to be played");
    click(&mut u, toggle);
    assert_eq!(video(&u, clip)["playing"], json!(true));
    u.frames(30, Vec::new);
    assert!(near(pos(&u, clip), 0.5 + 4.0 / 60.0), "{}", pos(&u, clip));
    click(&mut u, toggle);
    let at = pos(&u, clip);
    assert_eq!(video(&u, clip)["playing"], json!(false));
    u.frames(30, Vec::new);
    assert_eq!(pos(&u, clip), at, "paused: it stays");
    // Play and pause set it either way.
    click(&mut u, play);
    click(&mut u, play);
    assert_eq!(video(&u, clip)["playing"], json!(true));
    click(&mut u, pause);
    click(&mut u, pause);
    assert_eq!(video(&u, clip)["playing"], json!(false));
    // Playing from the end starts over.
    let end = button(&mut u.app.session, a, "End", 260.0, json!([{"type": "mediaSkipTo", "target": clip, "time": 3.0}]));
    u.frames(2, Vec::new);
    click(&mut u, end);
    assert_eq!(pos(&u, clip), 3.0);
    click(&mut u, play);
    u.frames(5, Vec::new);
    assert!(pos(&u, clip) < 0.2, "{}", pos(&u, clip));
}

#[test]
fn skip_forward_backward_and_to_a_time_stay_within_the_video() {
    let Scene { mut s, a, clip, .. } = scene(30, json!({"autoplay": false}));
    let skips: Vec<(f64, NodeId)> = [1.5, -1.0, 100.0, -100.0]
        .iter()
        .enumerate()
        .map(|(i, secs)| {
            (*secs, button(&mut s, a, &format!("Skip {i}"), 10.0 + 60.0 * i as f64, json!([{"type": "mediaSkip", "target": clip, "seconds": secs}])))
        })
        .collect();
    let to = button(&mut s, a, "To", 250.0, json!([{"type": "mediaSkipTo", "target": clip, "time": 2.25}]));
    let mut u = start(s, a);
    click(&mut u, skips[0].1);
    assert_eq!(pos(&u, clip), 1.5);
    click(&mut u, skips[1].1);
    assert_eq!(pos(&u, clip), 0.5);
    click(&mut u, skips[2].1);
    assert_eq!(pos(&u, clip), 3.0, "not past the end");
    click(&mut u, skips[3].1);
    assert_eq!(pos(&u, clip), 0.0, "not before the start");
    click(&mut u, to);
    assert_eq!(pos(&u, clip), 2.25);
    assert_eq!(video(&u, clip)["frame"], json!(22), "and the picture follows");
    // Playing on from there.
    let play = button(&mut u.app.session, a, "Play", 320.0, json!([{"type": "mediaPlayPause", "target": clip, "mode": "on"}]));
    u.frames(2, Vec::new);
    click(&mut u, play);
    u.frames(30, Vec::new);
    assert!(near(pos(&u, clip), 2.75 + 4.0 / 60.0), "{}", pos(&u, clip));
}

#[test]
fn mute_is_state_only() {
    let Scene { mut s, a, clip, .. } = scene(30, json!({"autoplay": false}));
    let toggle = button(&mut s, a, "Toggle", 20.0, json!([{"type": "mediaMuteUnmute", "target": clip}]));
    let mute = button(&mut s, a, "Mute", 100.0, json!([{"type": "mediaMuteUnmute", "target": clip, "mode": "on"}]));
    let unmute = button(&mut s, a, "Unmute", 180.0, json!([{"type": "mediaMuteUnmute", "target": clip, "mode": "off"}]));
    let mut u = start(s, a);
    let muted = |u: &Harness| video(u, clip)["muted"].clone();
    assert_eq!(muted(&u), json!(false));
    click(&mut u, toggle);
    assert_eq!(muted(&u), json!(true));
    click(&mut u, toggle);
    assert_eq!(muted(&u), json!(false));
    click(&mut u, mute);
    click(&mut u, mute);
    assert_eq!(muted(&u), json!(true));
    click(&mut u, unmute);
    assert_eq!(muted(&u), json!(false));
    assert_eq!((video(&u, clip)["playing"].clone(), pos(&u, clip)), (json!(false), 0.0), "nothing else moved");
    // A video that starts muted does.
    let Scene { s, a, clip, .. } = scene(30, json!({"muted": true}));
    let u = start(s, a);
    assert_eq!(video(&u, clip)["muted"], json!(true));
}

#[test]
fn a_hit_time_fires_once_per_crossing_and_again_after_a_restart() {
    let Scene { mut s, a, clip, .. } = scene(30, json!({}));
    let menu = frame(&mut s, "Menu", 1000.0);
    // Each time the video plays through 1 s, one more overlay opens; at its very start, another.
    for time in [1.0, 0.0] {
        ex(
            &mut s,
            "proto.interaction.add",
            json!({"id": clip, "trigger": {"type": "mediaHit", "time": time}, "action": {"type": "openOverlay", "dest": menu}}),
        );
    }
    let mut u = start(s, a);
    assert_eq!(overlays(&u), 1, "time 0 fires as it starts");
    u.frames(30, Vec::new);
    assert_eq!(overlays(&u), 1, "half a second in: not yet");
    u.frames(40, Vec::new);
    assert_eq!(overlays(&u), 2, "a second in");
    u.frames(60 * 2 + 20, Vec::new);
    assert_eq!(overlays(&u), 2, "and never again on the way to the end");
    // Restarting clears the overlays and the video starts over: both fire again.
    restart(&mut u, pos2(700.0, 450.0));
    u.frames(3, Vec::new);
    assert_eq!(overlays(&u), 1, "the start again");
    assert!(pos(&u, clip) < 0.1);
    u.frames(70, Vec::new);
    assert_eq!(overlays(&u), 2);
}

#[test]
fn skipping_does_not_fire_what_it_jumps_over_but_crossing_again_does() {
    let Scene { mut s, a, clip, .. } = scene(30, json!({"autoplay": false}));
    let menu = frame(&mut s, "Menu", 1000.0);
    ex(
        &mut s,
        "proto.interaction.add",
        json!({"id": clip, "trigger": {"type": "mediaHit", "time": 1.0}, "action": {"type": "openOverlay", "dest": menu}}),
    );
    let to = |s: &mut Session, name: &str, x: f64, t: f64| {
        button(s, a, name, x, json!([{"type": "mediaSkipTo", "target": clip, "time": t}, {"type": "mediaPlayPause", "target": clip, "mode": "on"}]))
    };
    let past = to(&mut s, "Past", 20.0, 2.0);
    let before = to(&mut s, "Before", 100.0, 0.5);
    let mut u = start(s, a);
    click(&mut u, past);
    u.frames(10, Vec::new);
    assert_eq!(overlays(&u), 0, "1 s was jumped over");
    click(&mut u, before);
    u.frames(50, Vec::new);
    assert_eq!(overlays(&u), 1, "crossed from 0.5 s");
}

#[test]
fn the_end_fires_once_and_a_loop_never_ends() {
    let end = |extra: Value, frames: u32| {
        let Scene { mut s, a, clip, .. } = scene(frames, extra);
        let menu = frame(&mut s, "Menu", 1000.0);
        ex(&mut s, "proto.interaction.add", json!({"id": clip, "trigger": {"type": "mediaEnd"}, "action": {"type": "openOverlay", "dest": menu}}));
        ex(
            &mut s,
            "proto.interaction.add",
            json!({"id": clip, "trigger": {"type": "mediaHit", "time": 0.5}, "action": {"type": "openOverlay", "dest": menu}}),
        );
        (start(s, a), clip)
    };
    // 1 s long: it ends once, however long the screen stays.
    let (mut u, clip) = end(json!({}), 10);
    u.frames(30, Vec::new);
    assert_eq!(overlays(&u), 1, "half a second in: the hit only");
    u.frames(60, Vec::new);
    assert_eq!((overlays(&u), video(&u, clip)["ended"].clone()), (2, json!(true)));
    u.frames(240, Vec::new);
    assert_eq!(overlays(&u), 2, "and not again");
    // Looping: it goes round, firing its hit each time and never its end.
    let (mut u, clip) = end(json!({"loop": true}), 10);
    u.frames(60 * 3 + 40, Vec::new);
    assert_eq!(
        (overlays(&u), video(&u, clip)["ended"].clone(), video(&u, clip)["playing"].clone()),
        (4, json!(false), json!(true)),
        "0.5, 1.5, 2.5 and 3.5 s"
    );
}

#[test]
fn a_screen_shown_again_starts_its_video_over() {
    let Scene { mut s, a, b, clip } = scene(30, json!({}));
    let go = button(&mut s, a, "Go", 20.0, json!([{"type": "navigate", "dest": b}]));
    let back = button(&mut s, b, "Back button", 20.0, json!([{"type": "back"}]));
    let mut u = start(s, a);
    u.frames(60, Vec::new);
    assert!(near(pos(&u, clip), 1.0));
    click(&mut u, go);
    assert_eq!(screen(&u), "B");
    assert_eq!(video(&u, clip), Value::Null, "its video isn't playing any more");
    u.frames(30, Vec::new);
    click(&mut u, back);
    assert_eq!(screen(&u), "A");
    assert!(pos(&u, clip) < 0.2, "{}", pos(&u, clip));
    u.frames(60, Vec::new);
    assert!(near(pos(&u, clip), 1.0 + 2.0 / 60.0) || near(pos(&u, clip), 1.0), "{}", pos(&u, clip));
    // Restarting the prototype does the same.
    restart(&mut u, pos2(700.0, 450.0));
    u.frames(2, Vec::new);
    assert!(pos(&u, clip) < 0.1);
}

#[test]
fn media_actions_act_on_the_layer_they_name_else_the_one_they_are_on() {
    let Scene { mut s, a, b, clip } = scene(30, json!({"autoplay": false}));
    let other = clip_in(&mut s, a);
    let elsewhere = self::clip(&mut s, b, "Away", 30, json!({"autoplay": false}));
    // Names the second video on A; another names a video that is on screen B; another a plain layer.
    let first = button(&mut s, a, "First", 20.0, json!([{"type": "mediaPlayPause", "target": other, "mode": "on"}]));
    let away = button(&mut s, a, "Away", 100.0, json!([{"type": "mediaPlayPause", "target": elsewhere, "mode": "on"}]));
    let plain = button(&mut s, a, "Plain", 180.0, json!([{"type": "mediaPlayPause", "target": a, "mode": "on"}]));
    // The video itself: a click on it plays it (no target: the layer it is on).
    ex(&mut s, "proto.interaction.add", json!({"id": clip, "trigger": {"type": "onClick"}, "action": {"type": "mediaPlayPause", "mode": "on"}}));
    let mut u = start(s, a);
    click(&mut u, first);
    assert_eq!((video(&u, other)["playing"].clone(), video(&u, clip)["playing"].clone()), (json!(true), json!(false)));
    click(&mut u, away);
    click(&mut u, plain);
    assert_eq!(video(&u, clip)["playing"], json!(false), "a video on another screen or a layer without one: nothing");
    assert_eq!(video(&u, elsewhere), Value::Null);
    click(&mut u, clip);
    assert_eq!(video(&u, clip)["playing"], json!(true), "the layer the interaction is on");
}

/// A second video on screen `a`, below the first.
fn clip_in(s: &mut Session, a: NodeId) -> NodeId {
    clip(s, a, "Other", 30, json!({"autoplay": false, "x": 200, "y": 20, "width": 160}))
}

#[test]
fn only_the_videos_area_is_redrawn_and_a_big_one_is_drawn_whole() {
    let Scene { s, a, clip, .. } = scene(30, json!({}));
    let mut u = start(s, a);
    u.frames(5, Vec::new);
    let (renders, patches) = (count(&u, "renders"), count(&u, "patches"));
    assert!(renders >= 1, "the screen is rendered with its video in it");
    u.frames(60, Vec::new);
    assert_eq!(count(&u, "renders"), renders, "the screen isn't rendered again");
    let moved = count(&u, "patches") - patches;
    assert!((8..=13).contains(&moved), "ten pictures a second: {moved}");
    // Paused, nothing is redrawn.
    let stop = button(&mut u.app.session, a, "Stop", 300.0, json!([{"type": "mediaPlayPause", "target": clip, "mode": "off"}]));
    u.frames(2, Vec::new);
    click(&mut u, stop);
    u.frames(3, Vec::new);
    let (renders, patches) = (count(&u, "renders"), count(&u, "patches"));
    u.frames(30, Vec::new);
    assert_eq!((count(&u, "renders"), count(&u, "patches")), (renders, patches), "paused: nothing is drawn");
    // A video covering the whole screen is more than a patch is worth: the screen is rendered anew.
    let Scene { s, a, .. } = scene(30, json!({"width": 400}));
    let mut u = start(s, a);
    u.frames(5, Vec::new);
    let renders = count(&u, "renders");
    u.frames(30, Vec::new);
    assert!(count(&u, "renders") > renders + 1, "{} then {}", renders, count(&u, "renders"));
}

#[test]
fn layers_that_only_follow_their_video_are_not_hotspots() {
    let Scene { mut s, a, clip, .. } = scene(30, json!({}));
    ex(&mut s, "proto.interaction.add", json!({"id": clip, "trigger": {"type": "mediaEnd"}, "action": {"type": "back"}}));
    let tap = button(&mut s, a, "Tap", 20.0, json!([{"type": "back"}]));
    let u = start(s, a);
    let hot: Vec<Value> = presenter(&u)["hotspots"].as_array().cloned().unwrap_or_default().iter().map(|h| h["id"].clone()).collect();
    assert_eq!(hot, vec![json!(tap)], "{hot:?}");
}

#[test]
fn a_shown_poster_waits_until_a_video_without_autoplay_plays() {
    let Scene { mut s, a, clip, .. } = scene(30, json!({"autoplay": false, "poster": 1.0}));
    let play = button(&mut s, a, "Play", 20.0, json!([{"type": "mediaPlayPause", "target": clip}]));
    let mut u = start(s, a);
    assert_eq!(video(&u, clip)["frame"], json!(10), "the poster picture, 1 s in");
    click(&mut u, play);
    u.frames(3, Vec::new);
    assert!(video(&u, clip)["frame"].as_u64().unwrap() <= 1, "it plays from the start: {}", video(&u, clip));
}

#[test]
fn an_overlays_videos_start_when_it_opens_and_stop_when_it_closes() {
    let Scene { mut s, a, clip, .. } = scene(30, json!({}));
    let menu = frame(&mut s, "Menu", 1000.0);
    let inner = self::clip(&mut s, menu, "Inner", 30, json!({}));
    let open = button(&mut s, a, "Open", 20.0, json!([{"type": "openOverlay", "dest": menu}]));
    let mut u = start(s, a);
    u.frames(60, Vec::new);
    assert_eq!(video(&u, inner), Value::Null);
    click(&mut u, open);
    assert_eq!(overlays(&u), 1);
    u.frames(30, Vec::new);
    assert!(near(pos(&u, inner), 0.5), "{}", pos(&u, inner));
    assert!(pos(&u, clip) > 1.0, "the screen under it plays on: {}", pos(&u, clip));
    // Closing it (back) drops its video; opening again starts it over.
    let close = button(&mut u.app.session, menu, "Close", 20.0, json!([{"type": "closeOverlay"}]));
    u.frames(3, Vec::new);
    click(&mut u, close);
    assert_eq!((overlays(&u), video(&u, inner)), (0, Value::Null));
}

#[test]
fn editing_the_file_keeps_a_video_playing_until_its_settings_change() {
    let Scene { s, a, clip, .. } = scene(30, json!({}));
    let mut u = start(s, a);
    u.frames(60, Vec::new);
    let before = pos(&u, clip);
    // Another layer's edit: the video plays on.
    u.run("node.set", json!({"id": a, "props": {"name": "Home"}}));
    u.frames(2, Vec::new);
    assert!(pos(&u, clip) >= before, "{} then {}", before, pos(&u, clip));
    // Its own settings: it starts over.
    u.run("video.set", json!({"ids": [clip], "loop": true}));
    u.frames(2, Vec::new);
    assert!(pos(&u, clip) < 0.1, "{}", pos(&u, clip));
    assert_eq!(video(&u, clip)["looping"], json!(true));
}

#[test]
fn a_video_that_is_missing_or_broken_is_not_played_and_nothing_breaks() {
    let Scene { mut s, a, clip, .. } = scene(30, json!({}));
    // Not in the file at all, and a file that doesn't open.
    let gone = rect(&mut s, a, "Gone", [200.0, 150.0, 50.0, 50.0]);
    ex(
        &mut s,
        "proto.interaction.add",
        json!({"id": gone, "trigger": {"type": "mediaHit", "time": 1}, "action": {"type": "mediaSkip", "seconds": 5}}),
    );
    ex(&mut s, "proto.interaction.add", json!({"id": gone, "trigger": {"type": "mediaEnd"}, "action": {"type": "mediaSkipTo", "time": 1e300}}));
    let hash = figcraft_format::image_hash(b"nothing to see");
    s.doc_mut()
        .unwrap()
        .images
        .insert(hash.clone(), figcraft_doc::ImageData { bytes: std::sync::Arc::new(b"nothing to see".to_vec()), width: 0, height: 0 });
    s.doc_mut().unwrap().get_mut(gone).unwrap().fills = vec![figcraft_color::Paint::video("not-there", figcraft_color::ImageScaleMode::Fill)];
    let broken = rect(&mut s, a, "Broken", [260.0, 150.0, 50.0, 50.0]);
    s.doc_mut().unwrap().get_mut(broken).unwrap().fills = vec![figcraft_color::Paint::video(hash, figcraft_color::ImageScaleMode::Fill)];
    let mut u = start(s, a);
    u.frames(30, Vec::new);
    let playing: Vec<Value> = presenter(&u)["media"]["videos"].as_array().cloned().unwrap_or_default().iter().map(|v| v["id"].clone()).collect();
    assert_eq!(playing, vec![json!(clip)], "only the video that opens plays");
}
