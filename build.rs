//! Kopiert die FFmpeg-DLLs (aus `%FFMPEG_DIR%\bin`) neben die erzeugte EXE,
//! damit `target\<profil>\framescope.exe` direkt startbar ist (portabel).

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    let (Some(ffmpeg), Some(out)) = (env::var_os("FFMPEG_DIR"), env::var_os("OUT_DIR")) else {
        return;
    };
    // OUT_DIR = target/<profil>/build/<crate>-<hash>/out → drei Ebenen hoch.
    let Some(target_dir) = PathBuf::from(out).ancestors().nth(3).map(PathBuf::from) else {
        return;
    };
    let Ok(entries) = fs::read_dir(PathBuf::from(ffmpeg).join("bin")) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll")) {
            if let Some(name) = path.file_name() {
                let _ = fs::copy(&path, target_dir.join(name));
            }
        }
    }
}
