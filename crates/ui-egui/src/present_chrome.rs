//! The presenter's own controls, beyond playing the prototype: the table of its keyboard shortcuts
//! (the keys, the Options menu's hints and the "?" list all read it), the shortcut list window and
//! stepping through the page's screens.
//!
//! A child module of `present`: it works on the player's private state.

use egui::{Align2, Context, CornerRadius, Id, Key, Modifiers, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_doc::proto::{Easing, Transition};
use figcraft_doc::{Document, NodeId};

use super::{Presenter, page_settings};
use crate::i18n::{tr, tr_noop};
use crate::theme::{Tokens, regular, semibold};

/// What a presenter shortcut does. Every key the presenter itself reacts to is one of these (the
/// prototype's own key triggers come first and win over all but the [`Chord::always`] chords).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Act {
    /// Back to the flow's start.
    Restart,
    /// The next way of sizing the prototype (fit, fit width, fill, 100%).
    NextSize,
    /// The next screen of the page, in reading order.
    Next,
    /// The previous screen.
    Prev,
    /// Full screen on or off.
    Fullscreen,
    /// Show or hide the comments on the presented screen.
    Comments,
    /// The shortcut list.
    Help,
    /// End the presentation.
    Close,
}

#[cfg(test)]
impl Act {
    /// Every act, in the order the list shows them (the table must hold each of them).
    pub const ALL: &'static [Act] = &[Act::Next, Act::Prev, Act::Restart, Act::NextSize, Act::Fullscreen, Act::Comments, Act::Help, Act::Close];
}

/// Which modifier keys a chord needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum With {
    /// None held.
    None,
    /// Cmd on macOS, Ctrl elsewhere (nothing else).
    #[allow(dead_code)] // (the Hide UI key, next)
    Cmd,
    /// Shift (nothing else).
    Shift,
    /// Whatever is held.
    Any,
}

impl With {
    pub fn matches(self, m: Modifiers) -> bool {
        match self {
            With::None => m.is_none(),
            With::Cmd => m.command && !m.shift && !m.alt,
            With::Shift => m.shift && !m.command && !m.alt && !m.ctrl,
            With::Any => true,
        }
    }
}

/// One way to press a shortcut.
#[derive(Clone, Copy, Debug)]
pub struct Chord {
    pub key: Key,
    pub with: With,
    /// As the keymap writes it (`Shift+C`, `Cmd+\`, `ArrowRight`); shown through the platform's
    /// notation. A test keeps it in step with `key` and `with`.
    pub spec: &'static str,
    /// Wins over the prototype's own key triggers.
    pub always: bool,
}

const fn chord(key: Key, with: With, spec: &'static str) -> Chord {
    Chord { key, with, spec, always: false }
}

/// A shortcut of the presenter: what it does and the keys that do it.
#[derive(Clone, Copy, Debug)]
pub struct Shortcut {
    pub act: Act,
    /// English; shown translated.
    pub label: &'static str,
    /// The list's section (English).
    pub group: &'static str,
    pub chords: &'static [Chord],
    /// Works in the preview pane too (while the pointer is over it).
    pub pane: bool,
}

/// Every shortcut of the presenter, in the order the list shows them. The key handling
/// ([`act_for`]), the Options menu's hints ([`spec`]) and the list ([`help_window`]) all read this
/// table, so they cannot disagree.
pub const SHORTCUTS: &[Shortcut] = &[
    Shortcut {
        act: Act::Next,
        label: tr_noop("Next screen"),
        group: tr_noop("Screens"),
        chords: &[chord(Key::ArrowRight, With::Any, "ArrowRight"), chord(Key::Space, With::None, "Space"), chord(Key::N, With::None, "N")],
        pane: true,
    },
    Shortcut {
        act: Act::Prev,
        label: tr_noop("Previous screen"),
        group: tr_noop("Screens"),
        chords: &[chord(Key::ArrowLeft, With::Any, "ArrowLeft")],
        pane: true,
    },
    Shortcut { act: Act::Restart, label: tr_noop("Restart"), group: tr_noop("Screens"), chords: &[chord(Key::R, With::None, "R")], pane: true },
    Shortcut {
        act: Act::NextSize,
        label: tr_noop("Next size option"),
        group: tr_noop("View"),
        chords: &[chord(Key::Z, With::None, "Z")],
        pane: false,
    },
    Shortcut { act: Act::Fullscreen, label: tr_noop("Full screen"), group: tr_noop("View"), chords: &[chord(Key::F, With::None, "F")], pane: false },
    Shortcut {
        act: Act::Comments,
        label: tr_noop("Comments"),
        group: tr_noop("Panels"),
        chords: &[chord(Key::C, With::None, "C"), Chord { always: true, ..chord(Key::C, With::Shift, "Shift+C") }],
        pane: false,
    },
    Shortcut {
        act: Act::Help,
        label: tr_noop("Keyboard shortcuts"),
        group: tr_noop("Panels"),
        chords: &[chord(Key::Questionmark, With::Any, "?")],
        pane: false,
    },
    Shortcut {
        act: Act::Close,
        label: tr_noop("Close the presentation"),
        group: tr_noop("Presentation"),
        chords: &[chord(Key::Escape, With::None, "Escape")],
        pane: false,
    },
];

/// The row of the table for `act`.
pub fn shortcut(act: Act) -> Option<&'static Shortcut> {
    SHORTCUTS.iter().find(|s| s.act == act)
}

/// The first way to press `act`, as the keymap writes it (for the menus' hints).
pub fn spec(act: Act) -> &'static str {
    shortcut(act).and_then(|s| s.chords.first()).map_or("", |c| c.spec)
}

/// The act a key press does: `pane` leaves out what only the full-window presenter has. Shift+/
/// is the `?` key of a US keyboard (other layouts already send `?`).
pub fn act_for(key: Key, mods: Modifiers, pane: bool) -> Option<Act> {
    let key = if key == Key::Slash && mods.shift { Key::Questionmark } else { key };
    SHORTCUTS.iter().filter(|s| s.pane || !pane).find(|s| s.chords.iter().any(|c| c.key == key && c.with.matches(mods))).map(|s| s.act)
}

/// The acts of the key presses this frame that win over the prototype's key triggers (consumed:
/// the prototype and the editor never see them).
pub fn priority_acts(ctx: &Context) -> Vec<Act> {
    let presses: Vec<(Key, Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    let mut out = Vec::new();
    for (key, mods) in presses {
        let always = SHORTCUTS.iter().find(|s| s.chords.iter().any(|c| c.always && c.key == key && c.with.matches(mods)));
        if let Some(s) = always
            && ctx.input_mut(|i| i.consume_key(mods, key))
        {
            out.push(s.act);
        }
    }
    out
}

/// Do `act` on the player `p`; false when it doesn't apply now (the key is then left alone).
/// `pane`: the player is the preview pane's.
pub fn run(p: &mut Presenter, act: Act, ctx: &Context, now: f64, pane: bool) -> bool {
    match act {
        Act::Restart => {
            p.restart(p.start, now);
            true
        }
        Act::NextSize => {
            // (A responsive frame filling the window has no size options.)
            let device = p.doc.as_ref().is_some_and(|d| page_settings(d, p.current).device().is_some());
            if pane || !(device || p.fit.is_none()) {
                return false;
            }
            p.set_scale(p.view.scale.next(device));
            true
        }
        Act::Next => step(p, true, now),
        Act::Prev => step(p, false, now),
        Act::Fullscreen => {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
            true
        }
        Act::Comments => {
            p.view.comments = !p.view.comments;
            true
        }
        Act::Help => {
            p.ux.help = !p.ux.help;
            true
        }
        // Escape is handled where the frame ends, after what else takes it ([`escape`]).
        Act::Close => false,
    }
}

/// What Escape does now: it closes the shortcut list first; true when it should end the
/// presentation.
pub fn escape(p: &mut Presenter) -> bool {
    if p.ux.help {
        p.ux.help = false;
        return false;
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Stepping through the screens.

/// The screens the arrow keys, Space and N step through: the top-level frames of the page holding
/// `current` (also those in sections), in reading order (rows top to bottom, then left to right),
/// whatever flows the page has (a flow is only a starting point).
pub fn sequence(doc: &Document, current: NodeId) -> Vec<NodeId> {
    doc.top_frames(doc.page_of(current).unwrap_or(current))
}

/// Show the next or previous screen of the [`sequence`] at once, keeping the history so Back
/// returns. False with an overlay open (the keys do nothing then), at either end, or when the
/// screen isn't one of the page's top-level frames.
pub fn step(p: &mut Presenter, forward: bool, now: f64) -> bool {
    if !p.overlays.is_empty() {
        return false;
    }
    let Some(base) = p.doc.as_ref() else { return false };
    let frames = sequence(base, p.current);
    let Some(i) = frames.iter().position(|f| *f == p.current) else { return false };
    let next = if forward { frames.get(i + 1) } else { i.checked_sub(1).and_then(|j| frames.get(j)) };
    let Some(n) = next.copied() else { return false };
    p.go(n, Transition::Instant, 0.0, Easing::Linear, now, true, false);
    true
}

// ---------------------------------------------------------------------------------------------
// The shortcut list.

/// The presenter's controls state that isn't a viewing option (those are in `PresentView`).
#[derive(Default)]
pub struct Ux {
    /// The shortcut list is open.
    pub help: bool,
}

/// One key of the list, as a keycap; returns its width.
fn keycap(ui: &Ui, at: egui::Pos2, text: &str, t: &Tokens, paint: bool) -> f32 {
    let galley = ui.painter().layout_no_wrap(text.to_string(), regular(11.0), t.text);
    let w = (galley.size().x + 12.0).max(22.0);
    if paint {
        let r = egui::Rect::from_min_size(at, vec2(w, 22.0));
        ui.painter().rect_filled(r, CornerRadius::same(4), t.field);
        ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
        ui.painter().galley(pos2(r.center().x - galley.size().x / 2.0, r.center().y - galley.size().y / 2.0), galley, t.text);
    }
    w
}

/// The shortcut list: every row of [`SHORTCUTS`] under its section, its keys as keycaps. Not
/// modal: the keys still work while it shows. Escape, the close button or `?` again closes it.
pub fn help_window(ctx: &Context, p: &mut Presenter) {
    if !p.ux.help {
        return;
    }
    let t = Tokens::get(ctx);
    let mut open = true;
    crate::dialogs::window(tr("Keyboard shortcuts"), ctx).id(Id::new("present_help")).open(&mut open).show(ctx, |ui| {
        ui.set_width(400.0);
        let mut group = "";
        for s in SHORTCUTS {
            if s.group != group {
                if !group.is_empty() {
                    ui.add_space(10.0);
                }
                group = s.group;
                ui.label(egui::RichText::new(tr(group)).font(semibold(11.0)).color(t.text_secondary));
                ui.add_space(2.0);
            }
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
            ui.painter().text(pos2(row.min.x, row.center().y), Align2::LEFT_CENTER, tr(s.label), regular(11.5), t.text);
            // The keys, right-aligned.
            let texts: Vec<String> = s.chords.iter().map(|c| crate::toolbar::pretty_shortcut(c.spec)).collect();
            let widths: Vec<f32> = texts.iter().map(|k| keycap(ui, row.min, k, &t, false)).collect();
            let mut x = row.max.x - widths.iter().sum::<f32>() - 4.0 * widths.len().saturating_sub(1) as f32;
            for (k, w) in texts.iter().zip(&widths) {
                keycap(ui, pos2(x, row.center().y - 11.0), k, &t, true);
                x += w + 4.0;
            }
        }
    });
    if !open {
        p.ux.help = false;
    }
}
