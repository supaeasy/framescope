//! Kopiert die FFmpeg-DLLs (aus `%FFMPEG_DIR%\bin`) neben die erzeugte EXE,
//! damit `target\<profil>\framescope.exe` direkt startbar ist (portabel).

use std::{env, fs, path::PathBuf};

fn main() {
    embed_windows_resources();
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
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
        {
            if let Some(name) = path.file_name() {
                let _ = fs::copy(&path, target_dir.join(name));
            }
        }
    }
}

/// Bettet Icon und Versionsinfo in die EXE ein (nur Windows-Ziele).
fn embed_windows_resources() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut parts: Vec<&str> = version.split('.').collect();
    parts.resize(4, "0");
    let comma = parts.join(",");
    embed_resource::compile(
        "assets/app.rc",
        [
            format!("VER_COMMA={comma}"),
            format!("VER_STR=\"{version}\""),
        ],
    )
    .manifest_optional()
    .ok();
}
