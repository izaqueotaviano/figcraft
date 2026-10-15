//! REST import of the open-link action: `openInNewTab` (the published schema's flag; a link opens
//! in a new tab unless the file says it doesn't). Fixtures are synthetic, written by hand from
//! the published schema.

use super::proto_variable_tests::at;
use super::*;
use figcraft_doc::proto::Action;
use serde_json::json;

fn import_link(action: Value) -> (Vec<Action>, Vec<String>) {
    let file = json!({"name": "Link", "document": {"id": "0:0", "type": "DOCUMENT", "children": [
        {"id": "0:1", "name": "Page", "type": "CANVAS", "children": [
            {"id": "1:1", "name": "Home", "type": "FRAME", "relativeTransform": at(0.0, 0.0), "size": {"x": 200, "y": 200}, "children": [
                {"id": "1:2", "name": "Button", "type": "RECTANGLE", "relativeTransform": at(10.0, 10.0), "size": {"x": 40, "y": 20},
                 "interactions": [{"trigger": {"type": "ON_CLICK"}, "actions": [action]}]}
            ]}
        ]}
    ]}});
    let (doc, warnings) = import_with(&file, None, &no_images).unwrap();
    let actions = doc.nodes().find(|n| n.name == "Button").and_then(|n| n.interactions.first()).map(|it| it.actions.clone()).unwrap_or_default();
    (actions, warnings)
}

#[test]
fn links_open_in_a_new_tab_unless_the_file_says_otherwise() {
    let url = "https://example.com/a";
    let link = |new_tab| Some(Action::OpenUrl { url: url.into(), new_tab });
    // (action JSON, imported new_tab, derivation)
    let cases = [
        (json!({"type": "URL", "url": url, "openInNewTab": true}), true, "the flag, on"),
        (json!({"type": "URL", "url": url, "openInNewTab": false}), false, "the flag, off: the same tab"),
        (json!({"type": "URL", "url": url}), true, "files without the flag (older exports) open in a new tab, as they always did"),
        (json!({"type": "URL", "url": url, "openInNewTab": null}), true, "null is no answer"),
        (json!({"type": "URL", "url": url, "openInNewTab": "false"}), true, "text is not a flag: the default"),
        (json!({"type": "URL", "url": url, "openInNewTab": 0}), true, "a number is not a flag: the default"),
    ];
    for (action, new_tab, why) in cases {
        let (got, warnings) = import_link(action.clone());
        assert_eq!(got.first().cloned(), link(new_tab), "{action}: {why} ({warnings:?})");
    }
    // A link without a URL is still skipped, flag or not.
    for action in [json!({"type": "URL", "openInNewTab": false}), json!({"type": "URL", "url": "  ", "openInNewTab": false})] {
        assert!(import_link(action.clone()).0.is_empty(), "{action}");
    }
}

#[test]
fn junk_link_flags_never_panic() {
    for flag in [json!(null), json!([]), json!({}), json!(1e300), json!("x"), json!([true])] {
        let (actions, _) = import_link(json!({"type": "URL", "url": "https://example.com", "openInNewTab": flag}));
        assert!(matches!(actions.first(), Some(Action::OpenUrl { new_tab: true, .. })), "{flag}");
    }
}
