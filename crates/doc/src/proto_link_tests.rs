//! The open-link action: opening in a new tab or in place, and which addresses open at all.

use crate::proto::*;

fn link(url: &str, new_tab: bool) -> Action {
    Action::OpenUrl { url: url.into(), new_tab }
}

#[test]
fn a_link_opens_in_a_new_tab_unless_it_says_otherwise() {
    // Files written before the setting existed read as opening in a new tab, as they always did.
    let old: Action = serde_json::from_str(r#"{"type":"openUrl","url":"https://example.com"}"#).unwrap();
    assert_eq!(old, link("https://example.com", true));
    // The setting round-trips; on is the default and isn't written (such files stay as they were).
    let on = serde_json::to_value(link("https://example.com", true)).unwrap();
    assert!(on.get("new_tab").is_none(), "{on}");
    let off = serde_json::to_value(link("https://example.com", false)).unwrap();
    assert_eq!(off["new_tab"], false);
    for a in [link("https://example.com", true), link("https://example.com", false), link("", false)] {
        let back: Action = serde_json::from_value(serde_json::to_value(&a).unwrap()).unwrap();
        assert_eq!(back, a);
    }
    // Commands and other tools spell it in camel case, or as the REST schema does.
    for key in ["newTab", "openInNewTab", "new_tab"] {
        let j = format!(r#"{{"type":"openUrl","url":"https://a.b","{key}":false}}"#);
        assert_eq!(serde_json::from_str::<Action>(&j).unwrap(), link("https://a.b", false), "{key}");
    }
    // Junk is an error (the command reports it), never a panic.
    for junk in [r#""no""#, "1", "null", "[]"] {
        let j = format!(r#"{{"type":"openUrl","url":"https://a.b","newTab":{junk}}}"#);
        assert!(serde_json::from_str::<Action>(&j).is_err(), "{junk}");
    }
}

#[test]
fn only_web_mail_and_phone_links_open() {
    // (what the file says, what opens (None: nothing), why)
    let cases: [(&str, Option<&str>, &str); 24] = [
        ("https://example.com/a?b=c#d", Some("https://example.com/a?b=c#d"), "web"),
        ("http://example.com", Some("http://example.com"), "plain http"),
        ("HTTPS://EXAMPLE.COM", Some("HTTPS://EXAMPLE.COM"), "schemes are case-insensitive"),
        ("  https://example.com  ", Some("https://example.com"), "trimmed"),
        ("mailto:me@example.com?subject=Hi", Some("mailto:me@example.com?subject=Hi"), "mail"),
        ("tel:+15551234567", Some("tel:+15551234567"), "phone"),
        ("sms:+15551234567", Some("sms:+15551234567"), "text message"),
        ("example.com", Some("https://example.com"), "no scheme: a web address"),
        ("example.com/a:b", Some("https://example.com/a:b"), "a colon after the host isn't a scheme"),
        ("www.example.com?q=a:b", Some("https://www.example.com?q=a:b"), "nor one in the query"),
        ("javascript:alert(1)", None, "script"),
        ("JaVaScRiPt:alert(1)", None, "script, any case"),
        ("java\tscript:alert(1)", None, "browsers drop tabs and newlines inside a scheme: control characters refuse it"),
        ("java\nscript:alert(1)", None, "newline"),
        ("\u{0}javascript:alert(1)", None, "NUL"),
        ("data:text/html,<script>alert(1)</script>", None, "data"),
        ("file:///etc/passwd", None, "local files"),
        ("ms-msdt:/id PCWDiagnostic", None, "handlers other software registers"),
        ("//evil.example/a", None, "scheme-relative"),
        (":80/x", None, "no scheme before the colon"),
        ("localhost:3000", None, "a host and port read as a scheme: say http://"),
        ("", None, "empty"),
        ("   ", None, "blank"),
        ("https://exa\u{7}mple.com", None, "a bell"),
    ];
    for (url, want, why) in cases {
        assert_eq!(web_link(url).as_deref(), want, "{url:?}: {why}");
    }
    // Long links are refused rather than handed on.
    let long = format!("https://example.com/{}", "a".repeat(MAX_LINK));
    assert_eq!(web_link(&long), None);
    assert!(web_link(&format!("https://example.com/{}", "a".repeat(MAX_LINK - 30))).is_some());
    // Multi-byte text doesn't trip the length check or any slicing.
    assert!(web_link(&"é".repeat(MAX_LINK + 1)).is_none());
    assert!(web_link("https://exämple.com/ü").is_some());
}

#[test]
fn junk_addresses_never_panic() {
    for u in ["\u{feff}", "::::", "a:b:c", "a:", ":", "/", "?", "#", "http:", "mailto:", "\u{202e}https://x", "https://x\u{85}", "😀:x", "ſ:x"] {
        let _ = web_link(u);
    }
}
