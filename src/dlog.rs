//! Optionales Ereignisprotokoll für die Fehlersuche (`FRAMESCOPE_LOG=1`): schreibt Zeitstempel und
//! Ereignisse nach `framescope-log-<PID>.txt` neben die EXE. Ohne die Variable passiert nichts.

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static LOG: OnceLock<Option<Mutex<(std::fs::File, Instant)>>> = OnceLock::new();

fn sink() -> &'static Option<Mutex<(std::fs::File, Instant)>> {
    LOG.get_or_init(|| {
        std::env::var_os("FRAMESCOPE_LOG")?;
        let exe = std::env::current_exe().ok()?;
        let path = exe.with_file_name(format!("framescope-log-{}.txt", std::process::id()));
        Some(Mutex::new((
            std::fs::File::create(path).ok()?,
            Instant::now(),
        )))
    })
}

/// Schreibt eine Zeile (`+Sekunden Text`), falls das Protokoll aktiv ist.
pub fn log(text: impl FnOnce() -> String) {
    if let Some(m) = sink() {
        if let Ok(mut g) = m.lock() {
            let secs = g.1.elapsed().as_secs_f64();
            let _ = writeln!(g.0, "+{secs:8.3} {}", text());
        }
    }
}
