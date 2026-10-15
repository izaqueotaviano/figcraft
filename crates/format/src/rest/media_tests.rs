//! REST import of media triggers (`ON_MEDIA_HIT`, `ON_MEDIA_END`) and the `UPDATE_MEDIA_RUNTIME`
//! action. Fixtures are synthetic, written by hand from the published schema.

use super::proto_variable_tests::at;
use super::*;
use figcraft_doc::proto::{Action, DEFAULT_SKIP, Interaction, MAX_MEDIA_SECONDS, MediaToggle, Trigger};
use serde_json::json;

/// A screen with a video-less button (`1:2`) and a second layer (`1:3`) the actions can point at;
/// the button has one interaction with `trigger` and `action`.
fn import(trigger: Value, action: Value) -> (Vec<Interaction>, Vec<String>, Option<figcraft_doc::NodeId>) {
    let file = json!({"name": "Media", "document": {"id": "0:0", "type": "DOCUMENT", "children": [
        {"id": "0:1", "name": "Page", "type": "CANVAS", "children": [
            {"id": "1:1", "name": "Home", "type": "FRAME", "relativeTransform": at(0.0, 0.0), "size": {"x": 200, "y": 200}, "children": [
                {"id": "1:2", "name": "Button", "type": "RECTANGLE", "relativeTransform": at(10.0, 10.0), "size": {"x": 40, "y": 20},
                 "interactions": [{"trigger": trigger, "actions": [action]}]},
                {"id": "1:3", "name": "Clip", "type": "RECTANGLE", "relativeTransform": at(10.0, 50.0), "size": {"x": 160, "y": 90}}
            ]}
        ]}
    ]}});
    let (doc, warnings) = import_with(&file, None, &no_images).unwrap();
    let list = doc.nodes().find(|n| n.name == "Button").map(|n| n.interactions.clone()).unwrap_or_default();
    let clip = doc.nodes().find(|n| n.name == "Clip").map(|n| n.id);
    (list, warnings, clip)
}

fn media(kind: &str, extra: Value) -> Value {
    let mut a = json!({"type": "UPDATE_MEDIA_RUNTIME", "mediaAction": kind});
    if let (Some(o), Value::Object(x)) = (a.as_object_mut(), extra) {
        o.extend(x);
    }
    a
}

#[test]
fn media_triggers_import_with_their_time() {
    let click = json!({"type": "ON_CLICK"});
    let hit = |t: Value| import(t, media("PLAY", json!({}))).0.first().map(|i| i.trigger.clone());
    assert_eq!(hit(json!({"type": "ON_MEDIA_HIT", "mediaHitTime": 2.5})), Some(Trigger::MediaHit { time: 2.5 }));
    assert_eq!(hit(json!({"type": "ON_MEDIA_HIT"})), Some(Trigger::MediaHit { time: 0.0 }), "no time: the start");
    assert_eq!(hit(json!({"type": "ON_MEDIA_HIT", "mediaHitTime": -4})), Some(Trigger::MediaHit { time: 0.0 }), "negative reads as the start");
    assert_eq!(hit(json!({"type": "ON_MEDIA_HIT", "mediaHitTime": 1e300})), Some(Trigger::MediaHit { time: MAX_MEDIA_SECONDS }));
    assert_eq!(hit(json!({"type": "ON_MEDIA_HIT", "mediaHitTime": "soon"})), Some(Trigger::MediaHit { time: 0.0 }));
    assert_eq!(hit(json!({"type": "ON_MEDIA_END"})), Some(Trigger::MediaEnd));
    // The other triggers are as they were.
    assert_eq!(hit(click), Some(Trigger::OnClick));
}

#[test]
fn media_actions_import_by_kind() {
    let (list, warnings, clip) = import(json!({"type": "ON_CLICK"}), media("PLAY", json!({"destinationId": "1:3"})));
    assert_eq!(list.first().map(|i| i.actions.clone()), Some(vec![Action::MediaPlayPause { target: clip, mode: MediaToggle::On }]), "{warnings:?}");
    assert!(clip.is_some());
    let one = |kind: &str, extra: Value| import(json!({"type": "ON_CLICK"}), media(kind, extra)).0.first().and_then(|i| i.actions.first().cloned());
    assert_eq!(one("PAUSE", json!({})), Some(Action::MediaPlayPause { target: None, mode: MediaToggle::Off }));
    assert_eq!(one("TOGGLE_PLAY_PAUSE", json!({})), Some(Action::MediaPlayPause { target: None, mode: MediaToggle::Toggle }));
    assert_eq!(one("MUTE", json!({})), Some(Action::MediaMuteUnmute { target: None, mode: MediaToggle::On }));
    assert_eq!(one("UNMUTE", json!({})), Some(Action::MediaMuteUnmute { target: None, mode: MediaToggle::Off }));
    assert_eq!(one("TOGGLE_MUTE_UNMUTE", json!({})), Some(Action::MediaMuteUnmute { target: None, mode: MediaToggle::Toggle }));
    assert_eq!(one("SKIP_FORWARD", json!({"amountToSkip": 10})), Some(Action::MediaSkip { target: None, seconds: 10.0 }));
    assert_eq!(one("SKIP_BACKWARD", json!({"amountToSkip": 10})), Some(Action::MediaSkip { target: None, seconds: -10.0 }));
    assert_eq!(one("SKIP_FORWARD", json!({})), Some(Action::MediaSkip { target: None, seconds: DEFAULT_SKIP }), "no amount: the default");
    assert_eq!(one("SKIP_TO", json!({"newTimestamp": 12.5})), Some(Action::MediaSkipTo { target: None, time: 12.5 }));
    assert_eq!(one("SKIP_TO", json!({})), Some(Action::MediaSkipTo { target: None, time: 0.0 }));
    // Names some tools write with a MEDIA_ prefix.
    assert_eq!(one("MEDIA_PLAY_PAUSE", json!({})), Some(Action::MediaPlayPause { target: None, mode: MediaToggle::Toggle }));
    assert_eq!(one("media_skip_backward", json!({"amountToSkip": 3})), Some(Action::MediaSkip { target: None, seconds: -3.0 }));
}

#[test]
fn media_actions_that_cannot_be_imported_are_reported_not_guessed() {
    let (list, warnings, _) = import(json!({"type": "ON_CLICK"}), media("REWIND", json!({})));
    assert!(list.is_empty(), "{list:?}");
    assert!(warnings.iter().any(|w| w.contains("media actions of unknown kinds")), "{warnings:?}");
    let (list, warnings, _) = import(json!({"type": "ON_CLICK"}), media("PLAY", json!({"destinationId": "9:99"})));
    assert!(list.is_empty(), "{list:?}");
    assert!(warnings.iter().any(|w| w.contains("aren't in the file")), "{warnings:?}");
    let (list, _, _) = import(json!({"type": "ON_CLICK"}), json!({"type": "UPDATE_MEDIA_RUNTIME"}));
    assert!(list.is_empty(), "no kind at all");
}

#[test]
fn junk_media_fields_never_panic() {
    let junk = [json!(null), json!([]), json!({}), json!(1e300), json!(-1e300), json!("x"), json!([true]), json!(true)];
    for v in junk {
        for (key, trig) in [("mediaHitTime", "ON_MEDIA_HIT"), ("amountToSkip", "ON_CLICK"), ("newTimestamp", "ON_CLICK")] {
            for kind in ["SKIP_TO", "SKIP_FORWARD", "SKIP_BACKWARD", "PLAY"] {
                let mut trigger = json!({"type": trig});
                let mut action = media(kind, json!({}));
                if key == "mediaHitTime" {
                    trigger[key] = v.clone();
                } else {
                    action[key] = v.clone();
                }
                action["destinationId"] = v.clone();
                let (list, _, _) = import(trigger, action);
                // Whatever came out has times inside the allowed range.
                for it in &list {
                    if let Trigger::MediaHit { time } = it.trigger {
                        assert!((0.0..=MAX_MEDIA_SECONDS).contains(&time), "{time}");
                    }
                    for a in &it.actions {
                        match a {
                            Action::MediaSkip { seconds, .. } => assert!(seconds.abs() <= MAX_MEDIA_SECONDS),
                            Action::MediaSkipTo { time, .. } => assert!((0.0..=MAX_MEDIA_SECONDS).contains(time)),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
