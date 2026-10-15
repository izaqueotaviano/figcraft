//! The media triggers and actions of the prototype model: how they are stored, the defaults old
//! and sparse data reads as, labels, and how they follow layers that are copied.

use std::collections::BTreeMap;

use crate::NodeId;
use crate::proto::*;

fn json(a: &Action) -> serde_json::Value {
    serde_json::to_value(a).unwrap()
}

#[test]
fn media_triggers_are_stored_by_name() {
    let hit = Trigger::MediaHit { time: 2.5 };
    assert_eq!(serde_json::to_value(&hit).unwrap(), serde_json::json!({"type": "mediaHit", "time": 2.5}));
    assert_eq!(serde_json::to_value(Trigger::MediaEnd).unwrap(), serde_json::json!({"type": "mediaEnd"}));
    for t in [hit, Trigger::MediaEnd, Trigger::MediaHit { time: 0.0 }] {
        assert_eq!(serde_json::from_value::<Trigger>(serde_json::to_value(&t).unwrap()).unwrap(), t);
    }
    // A hit without a time is at 0.
    assert_eq!(serde_json::from_str::<Trigger>(r#"{"type":"mediaHit"}"#).unwrap(), Trigger::MediaHit { time: 0.0 });
    assert_eq!(Trigger::MediaHit { time: 2.5 }.label(), "On media hit 2.5s");
    assert_eq!(Trigger::MediaHit { time: 30.0 }.label(), "On media hit 30s");
    assert_eq!(Trigger::MediaEnd.label(), "On media end");
    assert!(Trigger::MediaEnd.is_media() && Trigger::MediaHit { time: 1.0 }.is_media() && !Trigger::OnClick.is_media());
    // Media triggers have no keys and no delay.
    assert!(!Trigger::MediaEnd.fires_on_key("Space", KeyMods::default()));
}

#[test]
fn media_actions_are_stored_sparsely_and_read_with_defaults() {
    let n = NodeId(7);
    // Defaults aren't written.
    assert_eq!(json(&Action::MediaPlayPause { target: None, mode: MediaToggle::Toggle }), serde_json::json!({"type": "mediaPlayPause"}));
    assert_eq!(json(&Action::MediaMuteUnmute { target: None, mode: MediaToggle::Toggle }), serde_json::json!({"type": "mediaMuteUnmute"}));
    assert_eq!(
        json(&Action::MediaPlayPause { target: Some(n), mode: MediaToggle::Off }),
        serde_json::json!({"type": "mediaPlayPause", "target": 7, "mode": "off"})
    );
    assert_eq!(json(&Action::MediaSkip { target: None, seconds: -10.0 }), serde_json::json!({"type": "mediaSkip", "seconds": -10.0}));
    assert_eq!(json(&Action::MediaSkipTo { target: None, time: 3.0 }), serde_json::json!({"type": "mediaSkipTo", "time": 3.0}));
    // Sparse data reads with the defaults: toggling, the layer itself, five seconds, the start.
    let read = |s: &str| serde_json::from_str::<Action>(s).unwrap();
    assert_eq!(read(r#"{"type":"mediaPlayPause"}"#), Action::MediaPlayPause { target: None, mode: MediaToggle::Toggle });
    assert_eq!(read(r#"{"type":"mediaSkip"}"#), Action::MediaSkip { target: None, seconds: DEFAULT_SKIP });
    assert_eq!(read(r#"{"type":"mediaSkipTo"}"#), Action::MediaSkipTo { target: None, time: 0.0 });
    assert_eq!(
        read(r#"{"type":"mediaMuteUnmute","mode":"on","target":3}"#),
        Action::MediaMuteUnmute { target: Some(NodeId(3)), mode: MediaToggle::On }
    );
    // Round trips.
    for a in [
        Action::MediaPlayPause { target: Some(n), mode: MediaToggle::On },
        Action::MediaMuteUnmute { target: None, mode: MediaToggle::Off },
        Action::MediaSkip { target: Some(n), seconds: 12.5 },
        Action::MediaSkipTo { target: Some(n), time: 0.25 },
    ] {
        assert_eq!(serde_json::from_value::<Action>(json(&a)).unwrap(), a);
        assert!(a.is_media());
        assert_eq!(a.dest(), None, "media targets aren't destinations: no arrows are drawn to them");
    }
    // Junk is an error the command reports.
    for junk in [r#"{"type":"mediaSkip","seconds":"x"}"#, r#"{"type":"mediaPlayPause","mode":"sideways"}"#, r#"{"type":"mediaSkipTo","target":"a"}"#]
    {
        assert!(serde_json::from_str::<Action>(junk).is_err(), "{junk}");
    }
}

#[test]
fn labels_and_toggles() {
    let labels: Vec<&str> = [
        Action::MediaPlayPause { target: None, mode: MediaToggle::Toggle },
        Action::MediaMuteUnmute { target: None, mode: MediaToggle::Toggle },
        Action::MediaSkip { target: None, seconds: 5.0 },
        Action::MediaSkipTo { target: None, time: 0.0 },
    ]
    .iter()
    .map(Action::label)
    .collect();
    assert_eq!(labels, ["Play / pause media", "Mute / unmute media", "Skip forward / backward media", "Skip to media time"]);
    assert!(!Action::Back.is_media());
    // (what it is now) → what each setting makes it
    for (now, toggle, on, off) in [(false, true, true, false), (true, false, true, false)] {
        assert_eq!((MediaToggle::Toggle.apply(now), MediaToggle::On.apply(now), MediaToggle::Off.apply(now)), (toggle, on, off));
    }
    for (t, want) in
        [(0.0, "0"), (1.5, "1.5"), (30.0, "30"), (0.25, "0.25"), (0.004, "0"), (1.999, "2"), (f64::NAN, "0"), (f64::INFINITY, "0"), (1e12, "86400")]
    {
        assert_eq!(seconds_text(t), want, "{t}");
    }
    assert_eq!(media_time(5.0), Some(5.0));
    assert_eq!((media_time(-1.0), media_time(f64::NAN), media_time(1e300)), (Some(0.0), None, Some(MAX_MEDIA_SECONDS)));
}

#[test]
fn a_target_copied_with_its_controls_stays_theirs_and_one_left_behind_stays_put() {
    let map = BTreeMap::from([(NodeId(1), NodeId(11))]);
    let mut a = Action::MediaPlayPause { target: Some(NodeId(1)), mode: MediaToggle::Toggle };
    crate::remap_action(&mut a, &map);
    assert_eq!(a.media_target(), Some(NodeId(11)));
    let mut b = Action::MediaSkip { target: Some(NodeId(2)), seconds: 5.0 };
    crate::remap_action(&mut b, &map);
    assert_eq!(b.media_target(), Some(NodeId(2)));
    let mut c = Action::MediaSkipTo { target: None, time: 1.0 };
    crate::remap_action(&mut c, &map);
    assert_eq!(c.media_target(), None);
    c.set_media_target(Some(NodeId(5)));
    assert_eq!(c.media_target(), Some(NodeId(5)));
    let mut back = Action::Back;
    back.set_media_target(Some(NodeId(5)));
    assert_eq!(back, Action::Back);
}
