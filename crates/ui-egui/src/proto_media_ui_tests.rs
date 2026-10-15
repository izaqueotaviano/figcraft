//! The editor's side of media: new media actions aim at the screen's video, the interaction editor
//! shows media triggers and actions without trouble, summaries name the video, and the colour
//! picker draws a video fill's controls. Driven headless through egui (the harness is
//! `present_tests`').

use figcraft_doc::proto::{Action, DEFAULT_SKIP, MediaToggle, Trigger};
use figcraft_doc::{NodeId, NodeKind};
use figcraft_engine::Session;
use serde_json::{Value, json};

use super::media::{default_target, has_video, video_layers};
use super::{action_of, action_summary};
use crate::colorpicker::PickerTarget;
use crate::present_tests::{Ui, id};

/// Screen A with a video layer "Clip" and a button; screen B with a plain rectangle.
fn setup() -> (Session, NodeId, NodeId, NodeId, NodeId) {
    let mut s = Session::new();
    let ex = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap();
    let a = id(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 400, "height": 300, "name": "A"})));
    let b = id(&ex(&mut s, "frame.create", json!({"x": 500, "y": 0, "width": 400, "height": 300, "name": "B"})));
    let data = figcraft_media::testing::synthetic(64, 48, 30, 10);
    use base64::Engine as _;
    let clip = id(&ex(
        &mut s,
        "image.place",
        json!({"data": base64::engine::general_purpose::STANDARD.encode(data), "video": true, "parent": a, "name": "Clip", "x": 10, "y": 10}),
    ));
    let btn = id(&ex(&mut s, "shape.rectangle", json!({"x": 20, "y": 200, "width": 120, "height": 40, "parent": a, "name": "Button"})));
    let other = id(&ex(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 120, "height": 40, "parent": b, "name": "Plain"})));
    (s, a, clip, btn, other)
}

#[test]
fn new_media_actions_aim_at_the_screens_video() {
    let (s, a, clip, btn, other) = setup();
    let doc = s.doc().unwrap();
    let page = s.page().unwrap();
    assert!(has_video(doc, clip) && !has_video(doc, btn));
    assert_eq!(video_layers(doc, btn), vec![clip]);
    assert_eq!(video_layers(doc, a), vec![clip]);
    assert!(video_layers(doc, other).is_empty(), "the other screen has no video");
    // From the button: the video. From the video itself: itself (no target). From a screen without one: itself too.
    assert_eq!(default_target(doc, btn), Some(clip));
    assert_eq!(default_target(doc, clip), None);
    assert_eq!(default_target(doc, other), None);
    let make = |label: &str, node: NodeId| action_of(label, doc, page, node, None).unwrap();
    assert_eq!(make("Play / pause media", btn), Action::MediaPlayPause { target: Some(clip), mode: MediaToggle::Toggle });
    assert_eq!(make("Mute / unmute media", btn), Action::MediaMuteUnmute { target: Some(clip), mode: MediaToggle::Toggle });
    assert_eq!(make("Skip forward / backward media", btn), Action::MediaSkip { target: Some(clip), seconds: DEFAULT_SKIP });
    assert_eq!(make("Skip to media time", clip), Action::MediaSkipTo { target: None, time: 0.0 });
}

#[test]
fn summaries_name_what_the_action_does_and_to_which_video() {
    let (s, _, clip, ..) = setup();
    let doc = s.doc().unwrap();
    let said = |a: Action| action_summary(doc, &a);
    assert_eq!(said(Action::MediaPlayPause { target: Some(clip), mode: MediaToggle::Toggle }), "Play / pause media · Clip");
    assert_eq!(said(Action::MediaMuteUnmute { target: None, mode: MediaToggle::On }), "Mute / unmute media");
    assert_eq!(said(Action::MediaSkip { target: Some(clip), seconds: -5.0 }), "Skip media -5s · Clip");
    assert_eq!(said(Action::MediaSkip { target: None, seconds: 2.5 }), "Skip media 2.5s");
    assert_eq!(said(Action::MediaSkipTo { target: Some(clip), time: 12.0 }), "Skip media to 12s · Clip");
}

#[test]
fn the_interaction_editor_draws_media_triggers_and_actions() {
    let (mut s, _, clip, btn, _) = setup();
    let ex = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap();
    // The video's own triggers.
    ex(&mut s, "proto.interaction.add", json!({"id": clip, "trigger": {"type": "mediaHit", "time": 1.5}, "action": {"type": "mediaMuteUnmute"}}));
    ex(&mut s, "proto.interaction.add", json!({"id": clip, "trigger": {"type": "mediaEnd"}, "action": {"type": "mediaSkipTo", "time": 0}}));
    // The button's four actions in one interaction.
    ex(
        &mut s,
        "proto.interaction.add",
        json!({"id": btn, "actions": [
            {"type": "mediaPlayPause", "target": clip, "mode": "off"},
            {"type": "mediaMuteUnmute", "target": clip},
            {"type": "mediaSkip", "target": clip, "seconds": -3},
            {"type": "mediaSkipTo", "target": clip, "time": 4.5},
        ]}),
    );
    // A button with no video of its own and a video that is not in this document.
    ex(&mut s, "select.set", json!({"ids": [clip]}));
    let mut u = Ui::new(s);
    u.app.ui.right_tab = crate::RightTab::Prototype;
    for (node, k, paths) in [(clip, 0usize, vec![vec![0usize]]), (clip, 1, vec![vec![0]]), (btn, 0, vec![vec![0], vec![1], vec![2], vec![3]])] {
        u.app.session.execute("select.set", &json!({"ids": [node]})).unwrap();
        crate::proto_panel::open_editor(&u.ctx, Some((node, k)));
        let open = egui::Id::new(("proto_open_action", node, k));
        for path in paths {
            u.ctx.data_mut(|d| d.insert_temp(open, path));
            u.frames(3, Vec::new);
            assert_eq!(crate::proto_panel::editor(&u.ctx), Some((node, k)), "the editor stays open");
        }
    }
    // Triggers named as the panel lists them.
    let doc = u.app.session.doc().unwrap();
    let its = &doc.node(clip).unwrap().interactions;
    assert_eq!(its[0].trigger, Trigger::MediaHit { time: 1.5 });
    assert_eq!(its[1].trigger, Trigger::MediaEnd);
    assert_eq!(super::trigger_summary(&its[0].trigger), "On media hit 1.5s");
    assert_eq!(super::trigger_summary(&its[1].trigger), "On media end");
}

#[test]
fn the_picker_draws_a_video_fills_controls() {
    let (s, _, clip, btn, _) = setup();
    let mut u = Ui::new(s);
    for (node, expect_video) in [(clip, true), (btn, false)] {
        u.app.session.execute("select.set", &json!({"ids": [node]})).unwrap();
        u.app.ui.picker = Some(PickerTarget::Paint { fills: true, index: 0, ids: vec![node] });
        u.frames(3, Vec::new);
        // It stays open and the fill is as it was: drawing changes nothing.
        assert!(u.app.ui.picker.is_some());
        let n = u.app.session.doc().unwrap().node(node).unwrap();
        assert_eq!(matches!(n.fills.first().map(|f| &f.kind), Some(figcraft_color::PaintKind::Video(_))), expect_video);
        assert!(matches!(n.kind, NodeKind::Rectangle));
    }
    // A stroke's picker has no Video type, and still draws.
    u.app.ui.picker = Some(PickerTarget::Paint { fills: false, index: 0, ids: vec![clip] });
    u.frames(3, Vec::new);
}
