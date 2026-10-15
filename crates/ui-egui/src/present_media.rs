//! What the player does with interactions that leave the screens: open-link actions (a new tab or
//! the page itself) and, below them, video and other media. The player's own state and flow stay in
//! `present.rs`; this module holds the parts that grew beside them.

use super::Presenter;
use crate::FigApp;
use crate::i18n::trf;

/// After a player's frame: open the links its interactions asked for. Only web, mail and phone
/// links open (a file's link runs on the viewer's machine: `javascript:`, `file:` and the like are
/// refused, with a note saying so); each opens in a new tab, or in the page itself when its action
/// says so (only the web version has a page to replace; the desktop app opens the system browser
/// either way).
pub(super) fn open_links(app: &mut FigApp, p: &mut Presenter) {
    for (url, new_tab) in p.urls.drain(..) {
        let Some(link) = figcraft_doc::proto::web_link(&url) else {
            let shown: String = url.chars().take(80).collect();
            app.toast(trf("Didn't open the link “{url}”: only web, mail and phone links open", &[("url", &shown)]));
            continue;
        };
        if new_tab {
            app.services.open_url(&link);
        } else {
            app.services.open_url_here(&link);
        }
    }
}

#[cfg(test)]
#[path = "present_link_tests.rs"]
mod link_tests;
