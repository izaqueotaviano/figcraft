//! The open-link action through commands: opening in a new tab or in place (`newTab`), what the
//! file keeps, undo, the REST and `.fig` mappings, junk values.

use serde_json::{Value, json};

use crate::params::id_of;
use crate::{EngineError, Session};
use figcraft_doc::NodeId;
use figcraft_doc::proto::Action;

fn frame(s: &mut Session, x: f64) -> NodeId {
    id_of(&s.execute("frame.create", &json!({"x": x, "y": 0, "width": 100, "height": 200})).unwrap()["id"]).unwrap()
}

fn action(s: &Session, id: NodeId) -> Action {
    s.doc().unwrap().node(id).unwrap().interactions[0].actions[0].clone()
}

fn link(url: &str, new_tab: bool) -> Action {
    Action::OpenUrl { url: url.into(), new_tab }
}

#[test]
fn a_link_opens_in_a_new_tab_unless_set_otherwise() {
    let mut s = Session::new();
    let a = frame(&mut s, 0.0);
    // (the action's JSON, the link it makes, why)
    let cases = [
        (json!({"type": "openUrl", "url": "https://a.example"}), link("https://a.example", true), "by default"),
        (json!({"type": "openUrl", "url": "https://a.example", "newTab": false}), link("https://a.example", false), "off, in camel case"),
        (json!({"type": "openUrl", "url": "https://a.example", "new_tab": false}), link("https://a.example", false), "off, as the file spells it"),
        (json!({"type": "openUrl", "url": "https://a.example", "openInNewTab": false}), link("https://a.example", false), "off, as the REST schema"),
        (json!({"type": "openUrl", "url": "https://a.example", "newTab": true}), link("https://a.example", true), "on"),
    ];
    for (act, want, why) in cases {
        s.execute("proto.interaction.clear", &json!({"ids": [a]})).unwrap();
        s.execute("proto.interaction.add", &json!({"id": a, "action": act})).unwrap();
        assert_eq!(action(&s, a), want, "{act}: {why}");
    }
    // Flipped in place, one undo step per edit; the address stays.
    s.execute("proto.interaction.clear", &json!({"ids": [a]})).unwrap();
    s.execute("proto.interaction.add", &json!({"id": a, "action": {"type": "openUrl", "url": "https://b.example"}})).unwrap();
    s.execute("proto.interaction.set", &json!({"id": a, "index": 0, "action": {"type": "openUrl", "url": "https://b.example", "newTab": false}}))
        .unwrap();
    assert_eq!(action(&s, a), link("https://b.example", false));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(action(&s, a), link("https://b.example", true));
    // The nested form (a link inside a conditional's block) takes it too.
    let nested = json!({"type": "conditional", "blocks": [{"actions": [{"type": "openUrl", "url": "https://c.example", "newTab": false}]}]});
    s.execute("proto.interaction.add", &json!({"id": a, "action": nested})).unwrap();
    let Action::Conditional { blocks } = s.doc().unwrap().node(a).unwrap().interactions[1].actions[0].clone() else { panic!("not a conditional") };
    assert_eq!(blocks[0].actions, [link("https://c.example", false)]);
    // Saved in the file and read back.
    let bytes = figcraft_format::save_bytes(s.doc().unwrap(), None).unwrap();
    let (back, issues) = figcraft_format::load_bytes(&bytes).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(back.node(a).unwrap().interactions, s.doc().unwrap().node(a).unwrap().interactions);
}

#[test]
fn rest_carries_the_flag_and_fig_says_what_it_cannot() {
    let file = |extra: Value| {
        let mut act = json!({"type": "URL", "url": "https://rest.example"});
        act.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let at = json!([[1, 0, 0], [0, 1, 0]]);
        json!({"name": "File", "document": {"id": "0:0", "type": "DOCUMENT", "children": [{"id": "0:1", "name": "Page", "type": "CANVAS", "children": [
            {"id": "1:1", "name": "A", "type": "FRAME", "relativeTransform": at, "size": {"x": 100, "y": 100}, "interactions":
                [{"trigger": {"type": "ON_CLICK"}, "actions": [act]}], "children": []}]}]}})
    };
    for (extra, want) in [(json!({"openInNewTab": false}), false), (json!({"openInNewTab": true}), true), (json!({}), true)] {
        let (doc, warnings) = figcraft_format::rest::import(&file(extra.clone()), &figcraft_format::rest::no_images).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let frame = doc.nodes().find(|n| n.name == "A").unwrap();
        assert_eq!(frame.interactions[0].actions, [link("https://rest.example", want)], "{extra}");
        // `.fig` has no field the writer can verify: the link is written, and the file says so.
        let (_, warnings) = figcraft_format::fig::export(&doc, None).unwrap();
        assert_eq!(warnings.iter().any(|w| w.contains("same tab")), !want, "{extra}: {warnings:?}");
    }
}

#[test]
fn junk_flags_are_refused_not_fatal() {
    for j in [json!("no"), json!(0), json!(null), json!([true]), json!({"a": 1}), json!(f64::MAX)] {
        let mut s = Session::new();
        let a = frame(&mut s, 0.0);
        let act = json!({"type": "openUrl", "url": "https://a.example", "newTab": j});
        let r = s.execute("proto.interaction.add", &json!({"id": a, "action": act}));
        assert!(!matches!(r, Err(EngineError::Internal { .. })), "{j}: {r:?}");
        // Only a real boolean (or its absence) makes an interaction.
        let kept = s.doc().unwrap().node(a).unwrap().interactions.len();
        assert_eq!(kept, 0, "{j}: refused");
        let r = s.execute("proto.interaction.set", &json!({"id": a, "index": 0, "action": act}));
        assert!(!matches!(r, Err(EngineError::Internal { .. })), "set {j}: {r:?}");
    }
}
