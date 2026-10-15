//! Opened media shared by everything that shows the same file: the renderer draws a video's poster,
//! the player its moving pictures, each asking by the file's content hash. A file that doesn't open
//! is remembered too, so a bad file costs one parse, not one per frame.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::{Media, MediaError};

/// How many files stay open.
const KEPT: usize = 8;
/// How much the open files may hold together (bytes of decoded pictures and tables); the least
/// recently used go first.
const BUDGET: usize = 512 << 20;

type Entry = (String, Result<Arc<Media>, MediaError>, u64);

#[derive(Default)]
struct Open {
    tick: u64,
    entries: Vec<Entry>,
}

fn open_files() -> &'static Mutex<Open> {
    static OPEN: OnceLock<Mutex<Open>> = OnceLock::new();
    OPEN.get_or_init(|| Mutex::new(Open::default()))
}

/// The file with content hash `hash` and these `bytes`, opened once and kept for the next ask
/// (`bytes` must be what the hash names: the hash is the key).
pub fn shared(hash: &str, bytes: &Arc<Vec<u8>>) -> Result<Arc<Media>, MediaError> {
    {
        let mut o = open_files().lock().unwrap_or_else(PoisonError::into_inner);
        o.tick += 1;
        let tick = o.tick;
        if let Some(e) = o.entries.iter_mut().find(|e| e.0 == hash) {
            e.2 = tick;
            return e.1.clone();
        }
    }
    // Opened outside the lock: other files stay available meanwhile (a second ask for the same
    // file may open it twice; the first result stays).
    let opened = Media::open(bytes.clone()).map(Arc::new);
    let mut o = open_files().lock().unwrap_or_else(PoisonError::into_inner);
    o.tick += 1;
    let tick = o.tick;
    if let Some(e) = o.entries.iter().find(|e| e.0 == hash) {
        return e.1.clone();
    }
    o.entries.push((hash.to_string(), opened.clone(), tick));
    // Least recently used first out while there are too many or they hold too much; the newest stays.
    loop {
        let held: usize = o.entries.iter().filter_map(|e| e.1.as_ref().ok()).map(|m| m.resident_bytes()).fold(0, usize::saturating_add);
        if o.entries.len() <= 1 || (o.entries.len() <= KEPT && held <= BUDGET) {
            break;
        }
        let Some(oldest) = o.entries.iter().enumerate().filter(|(_, e)| e.2 != tick).min_by_key(|(_, e)| e.2).map(|(i, _)| i) else { break };
        let _gone = o.entries.swap_remove(oldest);
    }
    opened
}

/// Forget every opened file (tests; and when memory is wanted back).
pub fn forget_all() {
    open_files().lock().unwrap_or_else(PoisonError::into_inner).entries.clear();
}
