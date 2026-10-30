//! Broadcast input (docs/tools/ssh-sftp.md §6): what is typed into one
//! shell tab is also written to every other open shell of the same host.
//! Tabs are separate tools, so the writers meet in one process-wide
//! registry keyed by a per-tab serial; a tab registers when its shell
//! comes up and leaves when it disconnects or closes.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use hs_term::session::SessionWriter;
use parking_lot::Mutex;

static NEXT: AtomicU64 = AtomicU64::new(1);
static SHELLS: LazyLock<Mutex<HashMap<u64, (String, SessionWriter)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn next_serial() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub fn register(serial: u64, conn_id: &str, writer: SessionWriter) {
    SHELLS.lock().insert(serial, (conn_id.to_string(), writer));
}

pub fn unregister(serial: u64) {
    SHELLS.lock().remove(&serial);
}

/// Write `bytes` to every other registered shell of `conn_id`; how many
/// received it.
pub fn send(from: u64, conn_id: &str, bytes: &[u8]) -> usize {
    let shells = SHELLS.lock();
    let mut n = 0;
    for (serial, (conn, w)) in shells.iter() {
        if *serial != from && conn == conn_id {
            w.write(bytes);
            n += 1;
        }
    }
    n
}

/// How many other shells of `conn_id` are open, for the badge.
pub fn peers(from: u64, conn_id: &str) -> usize {
    SHELLS
        .lock()
        .iter()
        .filter(|(serial, (conn, _))| **serial != from && conn == conn_id)
        .count()
}
