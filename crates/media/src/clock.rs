//! The player: one video's playing state and clock, pure and deterministic.
//!
//! The caller owns time. It calls [`Player::advance`] with its own clock (the presenter's `now`,
//! a test's counter) and the player moves its position by how much that clock moved while it was
//! playing, never more than [`MAX_STEP`] at once (a window that was hidden or a frame that took
//! long doesn't make the video jump ahead). What a step crossed comes back as [`Events`]: the
//! "hit" times it passed and whether it ended or looped, each once per crossing.
//!
//! The rules (the public documentation of prototype media is thin; the unstated ones are ours):
//!
//! - Position stays within `0 ..= duration`: skipping and seeking clamp.
//! - Playing from the end starts over; a video that ended (not looping) stops at its end.
//! - A hit time fires when playing crosses it (`before < time <= after`), once per crossing: a
//!   skip or seek never fires the times it jumps over, going back and playing across a time
//!   again fires it again, and so does a loop. Time 0 fires when playing starts from the
//!   beginning. A time past the end never fires.
//! - A video that loops never ends: it fires its hit times again each time round and reports
//!   `looped` instead of `ended`.
//! - `ended` is reported once, when playing reaches the end; seeking or playing again re-arms it.
//! - Mute is state only: nothing is played, so nothing changes in the picture.

/// The most clock one [`Player::advance`] plays (seconds).
pub const MAX_STEP: f64 = 0.25;

/// A video's settings when it starts playing (a video fill's autoplay, loop and muted flags).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerConfig {
    /// Playing from the start (else paused until something plays it).
    pub autoplay: bool,
    pub looping: bool,
    pub muted: bool,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self { autoplay: true, looping: false, muted: false }
    }
}

/// What one [`Player::advance`] crossed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Events {
    /// Indexes into the `watch` list given to `advance` whose times were crossed, in time order
    /// (list order among equal times), each once.
    pub hits: Vec<usize>,
    /// Playing reached the end of a video that doesn't loop (it stopped there).
    pub ended: bool,
    /// Playing went round to the beginning of a video that loops.
    pub looped: bool,
}

impl Events {
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty() && !self.ended && !self.looped
    }
}

/// One video's playing state.
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    duration: f64,
    config: PlayerConfig,
    position: f64,
    playing: bool,
    muted: bool,
    looping: bool,
    ended: bool,
    /// The caller's clock at the last step, while playing.
    clock: Option<f64>,
    /// Where the next step's crossings start from: the position, or just before 0 so that time 0
    /// is crossed when playing starts from the beginning.
    edge: f64,
}

/// Before the beginning: the edge from which time 0 is crossed.
const BEFORE_START: f64 = -1.0;

impl Player {
    /// A player for a video of `duration` seconds (a duration that isn't a positive finite number
    /// is an empty video: it ends when first played).
    pub fn new(duration: f64, config: PlayerConfig) -> Player {
        let duration = if duration.is_finite() && duration > 0.0 { duration.min(crate::MAX_DURATION) } else { 0.0 };
        Player {
            duration,
            config,
            position: 0.0,
            playing: config.autoplay,
            muted: config.muted,
            looping: config.looping,
            ended: false,
            clock: None,
            edge: BEFORE_START,
        }
    }

    /// Back to the beginning with the settings it started with (a screen shown again, a prototype
    /// restarted).
    pub fn restart(&mut self) {
        *self = Player::new(self.duration, self.config);
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Seconds in, within `0 ..= duration`.
    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn looping(&self) -> bool {
        self.looping
    }

    /// It reached its end by playing and stopped there.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Play (from the beginning when it had ended).
    pub fn play(&mut self) {
        if self.ended || (self.position >= self.duration && !self.looping) {
            self.position = 0.0;
            self.edge = BEFORE_START;
            self.ended = false;
        }
        self.playing = true;
        self.clock = None;
    }

    pub fn pause(&mut self) {
        self.playing = false;
        self.clock = None;
    }

    /// Play when paused, pause when playing; true when it plays now.
    pub fn toggle_playing(&mut self) -> bool {
        if self.playing {
            self.pause()
        } else {
            self.play()
        }
        self.playing
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Mute when sounding, unmute when muted; true when muted now.
    pub fn toggle_muted(&mut self) -> bool {
        self.muted = !self.muted;
        self.muted
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// Jump to `t` seconds (clamped to the video; not a number: nothing). Crossings count from
    /// there on; the times it jumped over don't fire.
    pub fn seek(&mut self, t: f64) {
        if t.is_nan() {
            return;
        }
        self.position = t.clamp(0.0, self.duration);
        self.edge = if self.position <= 0.0 { BEFORE_START } else { self.position };
        self.ended = false;
    }

    /// Jump `seconds` ahead (back when negative).
    pub fn skip(&mut self, seconds: f64) {
        if seconds.is_nan() {
            return;
        }
        // (`seek` clamps; an infinite skip goes to the end or the start.)
        self.seek(self.position + seconds);
    }

    /// Move to the caller's clock `now`: while playing, the position follows how far the clock
    /// moved since the last call (at most [`MAX_STEP`]); the first call after playing starts
    /// only notes the clock. `watch` lists the hit times to report (indexes come back in
    /// [`Events::hits`]); times that aren't finite, are negative or lie past the end never fire.
    pub fn advance(&mut self, now: f64, watch: &[f64]) -> Events {
        let mut ev = Events::default();
        if !now.is_finite() {
            return ev;
        }
        if !self.playing {
            self.clock = None;
            return ev;
        }
        let dt = self.clock.map_or(0.0, |c| (now - c).clamp(0.0, MAX_STEP));
        self.clock = Some(now);
        if self.duration <= 0.0 {
            // An empty video ends the moment it plays.
            self.stop_at_end(&mut ev);
            return ev;
        }
        let next = self.position + dt;
        if next < self.duration {
            self.cross(watch, next, &mut ev);
            self.position = next;
            self.edge = next;
            return ev;
        }
        // Up to the end, and then it stops or goes round.
        self.cross(watch, self.duration, &mut ev);
        if self.looping {
            ev.looped = true;
            // A step longer than the video goes round more than once.
            let over = (next - self.duration) % self.duration;
            self.position = over;
            self.edge = BEFORE_START;
            self.cross(watch, over, &mut ev);
            // (Time 0 fired above when it was crossed: the next step starts after it.)
            self.edge = over;
        } else {
            self.stop_at_end(&mut ev);
        }
        ev
    }

    fn stop_at_end(&mut self, ev: &mut Events) {
        self.position = self.duration;
        self.edge = self.duration;
        self.playing = false;
        self.clock = None;
        self.ended = true;
        ev.ended = true;
    }

    /// Add to `ev` the watch times in `(edge, to]` not reported yet by this step.
    fn cross(&self, watch: &[f64], to: f64, ev: &mut Events) {
        let mut found: Vec<(f64, usize)> = watch
            .iter()
            .enumerate()
            .filter(|(i, t)| t.is_finite() && **t >= 0.0 && **t > self.edge && **t <= to && !ev.hits.contains(i))
            .map(|(i, t)| (*t, i))
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        ev.hits.extend(found.into_iter().map(|(_, i)| i));
    }
}
