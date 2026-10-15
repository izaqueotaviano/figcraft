//! The presenter's own controls, beyond playing the prototype: the table of its keyboard shortcuts
//! (the keys, the Options menu's hints and the "?" list all read it), the shortcut list window and
//! stepping through the page's screens.
//!
//! A child module of `present`: it works on the player's private state.

use egui::{Align2, Color32, Context, CornerRadius, Id, Key, Modifiers, Painter, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_doc::proto::{Easing, Transition};
use figcraft_doc::{Document, NodeId};
use serde_json::{Value, json};

use super::{PresentView, Presenter, page_settings};
use crate::FigApp;
use crate::a11y::Tip;
use crate::i18n::{tr, tr_noop, trf};
use crate::icons::{self, Icon};
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
    /// Hide or show the presenter's controls.
    HideUi,
    /// End the presentation.
    Close,
}

#[cfg(test)]
impl Act {
    /// Every act, in the order the list shows them (the table must hold each of them).
    pub const ALL: &'static [Act] =
        &[Act::Next, Act::Prev, Act::Restart, Act::NextSize, Act::Fullscreen, Act::HideUi, Act::Comments, Act::Help, Act::Close];
}

/// Which modifier keys a chord needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum With {
    /// None held.
    None,
    /// Cmd on macOS, Ctrl elsewhere (nothing else).
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
        act: Act::HideUi,
        label: tr_noop("Hide UI"),
        group: tr_noop("View"),
        chords: &[Chord { always: true, ..chord(Key::Backslash, With::Cmd, "Cmd+\\") }],
        pane: false,
    },
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
        Act::HideUi => {
            p.view.hide_ui = !p.view.hide_ui;
            p.ux.reveal = 0.0;
            true
        }
        // Escape is handled where the frame ends, after what else takes it ([`escape`]).
        Act::Close => false,
    }
}

/// What Escape does now: it closes the shortcut list first, then shows the hidden controls
/// again; true when it should end the presentation (nothing was left to close or show).
pub fn escape(p: &mut Presenter) -> bool {
    if p.ux.help {
        p.ux.help = false;
        return false;
    }
    if p.view.hide_ui {
        p.view.hide_ui = false;
        p.ux.reveal = 0.0;
        return false;
    }
    true
}

/// How long the hidden bar takes to slide all the way in or out (seconds).
const SLIDE_SECS: f64 = 0.14;
/// How close to the window's top edge the pointer must be to ask for the hidden bar (points).
const EDGE: f32 = 4.0;

/// With the controls hidden: slide the bar in while the pointer is at the window's top edge (or
/// over the bar, or one of its menus is open), out again when it isn't. Returns where the bar is
/// now, `None` while it is away; the caller draws it over the prototype. A drag that began in the
/// prototype doesn't pull the bar in as it passes the edge. `rect` is the whole window.
pub fn peek(p: &mut Presenter, ctx: &Context, rect: Rect, bar_h: f32, now: f64) -> Option<Rect> {
    let (pos, down) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_down()));
    let shown = p.ux.reveal > 0.0;
    let zone = if shown { bar_h + 6.0 } else { EDGE };
    let near = pos.is_some_and(|q| q.x >= rect.min.x && q.x <= rect.max.x && q.y >= rect.min.y - 1.0 && q.y <= rect.min.y + zone);
    let want = (shown && egui::Popup::is_any_open(ctx)) || (near && (shown || !down));
    // Steps of at most a frame at 30 Hz, so a window that was asleep slides in rather than jumps.
    let dt = (now - p.ux.reveal_at).clamp(0.0, 1.0 / 30.0);
    p.ux.reveal_at = now;
    let step = (dt / SLIDE_SECS) as f32;
    p.ux.reveal = (p.ux.reveal + if want { step } else { -step }).clamp(0.0, 1.0);
    if (want && p.ux.reveal < 1.0) || (!want && p.ux.reveal > 0.0) {
        ctx.request_repaint();
    }
    if p.ux.reveal <= 0.0 {
        return None;
    }
    let t = p.ux.reveal;
    let e = t * t * (3.0 - 2.0 * t);
    Some(Rect::from_min_size(pos2(rect.min.x, rect.min.y - bar_h * (1.0 - e)), vec2(rect.width(), bar_h)))
}

// ---------------------------------------------------------------------------------------------
// The control channel.

/// The presenter's viewing options as `ui.inspect` shows them (`presentOptions`): what is kept for
/// the next presentation.
pub fn options_json(v: &PresentView) -> Value {
    json!({
        "scale": v.scale.id(),
        "deviceFrame": v.device_frame,
        "comments": v.comments,
        "hotspotHints": v.hotspot_hints,
        "inspect": v.inspect,
        "responsive": v.responsive,
        "hideUi": v.hide_ui,
    })
}

/// Apply `f` to the options kept for the next presentation and to the presentation playing.
fn both(app: &mut FigApp, f: impl Fn(&mut PresentView, &mut Ux)) {
    f(&mut app.ui.present_view, &mut Ux::default());
    if let Some(p) = app.ui.present.as_mut() {
        f(&mut p.view, &mut p.ux);
    }
}

/// `ui.set` keys for the presenter's controls (`proto.presentOptions` has the sizing ones):
/// `presentHideUi` (true|false). They change the presentation playing and what the next one starts
/// with.
pub fn set_options(app: &mut FigApp, p: &Value) -> Result<(), String> {
    let flag = |key: &str| -> Result<Option<bool>, String> {
        match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("{key}: true|false")),
        }
    };
    if let Some(on) = flag("presentHideUi")? {
        both(app, |v, ux| {
            v.hide_ui = on;
            ux.reveal = 0.0;
        });
    }
    Ok(())
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

/// Where the presented screen is in the [`sequence`]: its place counted from 1 and how many
/// screens there are. This is the "3 / 12" of the bar. It counts the page's top-level frames in
/// reading order, as the keys step through them, not the screens a flow reaches (a flow is only a
/// starting point, and several flows share screens). An overlay open over the screen doesn't change
/// it (the keys do nothing while one is open, and the bar's arrows follow). `None` when the page has
/// just this one screen, or the presented frame isn't among them: the bar then shows no counter.
pub fn position(p: &Presenter) -> Option<(usize, usize)> {
    let doc = p.doc.as_ref()?;
    let frames = sequence(doc, p.current);
    let i = frames.iter().position(|f| *f == p.current)?;
    (frames.len() > 1).then_some((i + 1, frames.len()))
}

/// The colours the bar's left side draws with (the full-window bar is dark, the pane's follows the
/// theme).
#[derive(Clone, Copy)]
pub struct BarColors {
    pub text: Color32,
    pub dim: Color32,
    pub hover: Color32,
}

/// The left end of the bar after the flow picker: the screen's name, then "3 / 12" between
/// previous and next buttons. `left` paints (clipped to what the buttons at the right leave), `from`
/// is where the name starts and `limit` where the buttons at the right begin. The counter keeps its
/// room and the name is cut to what is left (at least [`MIN_NAME`] of it); where even that is too
/// tight the buttons go and the text stays, and where the text doesn't fit either, it goes.
#[allow(clippy::too_many_arguments)]
pub fn name_and_counter(
    p: &mut Presenter,
    ui: &Ui,
    left: &Painter,
    bar: Rect,
    from: f32,
    limit: f32,
    name: &str,
    name_color: Color32,
    c: BarColors,
    now: f64,
) {
    const BTN: f32 = 22.0;
    /// The least of the name the counter leaves (points).
    const MIN_NAME: f32 = 48.0;
    let cy = bar.center().y;
    let counter = position(p).map(|(i, n)| (format!("{i} / {n}"), i, n));
    let galley = |t: &str| left.layout_no_wrap(t.to_string(), regular(11.5), c.dim);
    let text_w = counter.as_ref().map_or(0.0, |(t, ..)| galley(t).size().x);
    let room = limit - from - 8.0 - MIN_NAME;
    let buttons = room >= BTN + 4.0 + text_w + 4.0 + BTN;
    let shown = counter.filter(|_| buttons || room >= text_w);
    let cluster_w = shown.as_ref().map_or(0.0, |_| if buttons { BTN + 4.0 + text_w + 4.0 + BTN } else { text_w });
    // The counter sits after the name, or at the right end when the name is long.
    let x0 = if shown.is_some() { (from + galley(name).size().x + 10.0).min(limit - cluster_w - 8.0) } else { limit };
    let name_clip = Rect::from_min_max(bar.min, pos2((x0 - 6.0).max(from), bar.max.y));
    left.with_clip_rect(name_clip.intersect(left.clip_rect())).text(pos2(from, cy), Align2::LEFT_CENTER, name, regular(11.5), name_color);
    let Some((text, i, n)) = shown else { return };
    let stepping = p.overlays.is_empty();
    let button = |p: &mut Presenter, at: f32, forward: bool, enabled: bool| {
        let r = Rect::from_min_size(pos2(at, cy - BTN / 2.0), vec2(BTN, BTN));
        p.ux.note(if forward { "next" } else { "prev" }, r);
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let id = ui.id().with(("pbar_step", forward));
        let resp = ui.interact(r.intersect(left.clip_rect()), id, sense);
        let label = if forward { tr("Next screen") } else { tr("Previous screen") };
        let key = ux_spec(if forward { Act::Next } else { Act::Prev });
        let resp = resp.tip(format!("{label} ({key})"));
        if enabled && resp.hovered() {
            left.rect_filled(r, CornerRadius::same(5), c.hover);
        }
        let icon = if forward { Icon::ChevronRight } else { Icon::ChevronLeft };
        icons::paint(left, r.shrink(5.0), icon, if enabled { c.text } else { c.dim.gamma_multiply(0.5) });
        if enabled && resp.clicked() {
            step(p, forward, now);
        }
    };
    let tx = if buttons { x0 + BTN + 4.0 } else { x0 };
    if buttons {
        button(p, x0, false, stepping && i > 1);
    }
    let label = left.text(pos2(tx, cy), Align2::LEFT_CENTER, &text, regular(11.5), c.dim);
    p.ux.note("position", label);
    let tip = ui.interact(label.expand2(vec2(2.0, 8.0)).intersect(left.clip_rect()), ui.id().with("pbar_position"), Sense::hover());
    let _ = tip.on_hover_text(trf("Screen {x} of {y}", &[("x", &i), ("y", &n)]));
    if buttons {
        button(p, tx + text_w + 4.0, true, stepping && i < n);
    }
}

/// [`spec`] as the platform writes it.
fn ux_spec(act: Act) -> String {
    crate::toolbar::pretty_shortcut(spec(act))
}

// ---------------------------------------------------------------------------------------------
// The shortcut list.

/// The presenter's controls state that isn't a viewing option (those are in `PresentView`).
#[derive(Default)]
pub struct Ux {
    /// The shortcut list is open.
    pub help: bool,
    /// Where the controls of the bar (and the panels) were drawn last, by name (window points):
    /// `ui.inspect` reports them so that a script can click them.
    pub controls: Vec<(&'static str, Rect)>,
    /// How far the hidden bar has slid in, 0 (away) to 1 (all the way), and when it last moved.
    pub reveal: f32,
    pub reveal_at: f64,
}

impl Ux {
    /// Note where the control `name` is drawn.
    pub fn note(&mut self, name: &'static str, r: Rect) {
        self.controls.push((name, r));
    }
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
