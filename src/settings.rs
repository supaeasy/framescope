//! Portable Einstellungen: eine einfache `key=value`-Datei neben der EXE.
//! Es werden keine Registry-Einträge und keine Dateien in `%APPDATA%` angelegt.

use std::path::PathBuf;

const FILE_NAME: &str = "framescope.ini";

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub volume: f32,
    pub muted: bool,
    /// Dauerhaftes Info-Overlay (Frame, Timecode, KEY) oben links.
    pub hud: bool,
    /// Standardordner für den PNG-Export (`None` = beim ersten Export fragen).
    pub export_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 0.8,
            muted: false,
            hud: false,
            export_dir: None,
        }
    }
}

fn file_path() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join(FILE_NAME))
}

impl Settings {
    /// Lädt die Datei; fehlende oder defekte Einträge fallen auf Standardwerte zurück.
    pub fn load() -> Self {
        let text = file_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "volume" => {
                    if let Ok(v) = value.parse::<f32>() {
                        s.volume = v.clamp(0.0, 1.0);
                    }
                }
                "muted" => s.muted = value == "true",
                "hud" => s.hud = value == "true",
                "export_dir" if !value.is_empty() => s.export_dir = Some(PathBuf::from(value)),
                _ => {}
            }
        }
        s
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("# FrameScope – Einstellungen (portabel, neben der EXE)\n");
        out.push_str(&format!(
            "volume={:.2}\nmuted={}\nhud={}\n",
            self.volume, self.muted, self.hud
        ));
        if let Some(d) = &self.export_dir {
            out.push_str(&format!("export_dir={}\n", d.display()));
        }
        out
    }

    /// Schreibt die Datei. Ist der Ordner nicht beschreibbar, wird still verzichtet.
    pub fn save(&self) {
        if let Some(p) = file_path() {
            let _ = std::fs::write(p, self.serialize());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Settings {
            volume: 0.35,
            muted: true,
            hud: true,
            export_dir: Some(PathBuf::from("C:\\Frames")),
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
    }

    #[test]
    fn defaults_on_garbage() {
        let s = Settings::parse("volume=laut\nmuted\n=\nexport_dir=\n# kommentar\nfoo=bar");
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn volume_is_clamped() {
        assert_eq!(Settings::parse("volume=7").volume, 1.0);
    }
}
