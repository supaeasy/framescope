# FrameScope – Plan

Schlanker, portabler Videoplayer für Windows 11 mit Fokus auf **frame-genaue Analyse und Vergleich**.

## Technologie-Entscheidungen

| Bereich | Wahl | Begründung |
|---|---|---|
| Decoding | FFmpeg via `ffmpeg-next` (LGPL, shared) | Einzige Option mit vollständiger Format-Abdeckung (H.264/H.265/MKV/MOV/WebM/AVI), Paket-Flags (`KEY`) und exaktem Seeking. Alternativen: *GStreamer* (schwergewichtig, schwieriger portabel), *Media Foundation* (Format-Lücken, kaum Keyframe-Info), *symphonia/rav1e* (nur Audio bzw. einzelne Codecs). |
| UI | `eframe`/`egui` (glow-Backend) | Immediate-Mode passt zu einem Overlay-UI mit Timeline-Eigenbau; geringer Footprint, schneller Start. glow statt wgpu: kleineres Binary, schnellerer Start. |
| Audio | `cpal` (WASAPI) + `rubato` | Direkter Zugriff auf Ausgabe-Callback → Audio-Clock als Master. Resampling auf Gerätesamplerate. |
| Fehler | `anyhow` (App) / `thiserror` (Module) | Keine `unwrap()` im Produktivcode. |
| Channels | `crossbeam-channel` | Bounded Channels für Backpressure Decoder → UI. |
| Dialoge | `rfd` | Native Datei-/Ordner-Dialoge. |
| PNG-Export | `image` (nur PNG-Feature) | Volle Originalauflösung aus dem dekodierten Frame (swscale → RGBA). |

### Lizenz
FFmpeg wird als **LGPL-Build, dynamisch gelinkt** genutzt (DLLs im ZIP, keine GPL-Komponenten wie x264/x265). Projektlizenz: eigene Source-Available-Lizenz „Kein Verkauf ohne Genehmigung“ (siehe `LICENSE`; ab v0.2.1 – v0.2.0 und älter standen unter MIT), kompatibel zu dynamischem LGPL-Linking. Lizenztexte/Hinweise liegen dem Release bei (`THIRD-PARTY-LICENSES.md`).

## Architektur

```
 ┌───────────┐  Command (Open/Play/Seek/Step…)   ┌────────────────┐
 │  UI/App   │ ────────────────────────────────▶ │ Decoder-Thread │
 │ (egui)    │ ◀──────────────────────────────── │  (ffmpeg)      │
 └─────┬─────┘  Frame{idx,pts,key,rgba}, Events  └───────┬────────┘
       │ Clock                                           │ Audio-Samples
       ▼                                                 ▼
 ┌───────────┐          Audio-Master-Clock        ┌───────────────┐
 │ Timeline  │ ◀───────────────────────────────── │ Audio (cpal)  │
 └───────────┘                                    └───────────────┘
        ▲  Keyframe-Index (Hintergrund-Scan der Pakete, ohne Decoding)
```

Module: `app` (Zustand, Eingabe, Shortcuts), `decoder` (Öffnen, Decode-Loop, Seek/Step, Frame-Cache), `index` (Paket-Scan: Keyframes, PTS-Reihenfolge, Framecount), `audio`, `timeline` (Widget), `timecode` (Frame/Zeit-Rechnung, getestet), `export` (PNG), `settings` (Datei neben der EXE), `ui` (Theme, Overlay-Controls).

### Frame-Genauigkeit
- Der **Index-Scan** liest alle Video-Pakete (kein Decoding), sortiert die PTS-Werte → Frame *n* = n-ter Eintrag in Präsentationsreihenfolge. Das ist auch bei **VFR** exakt (Framecount = Paketzahl).
- Timecode `hh:mm:ss:ff`: Sekunde aus PTS, `ff` = Frame-Position innerhalb der Sekunde (bei VFR: laufender Zähler der Frames in dieser Sekunde).
- **Seek auf Frame n**: Seek zum Keyframe ≤ PTS(n), vorwärts dekodieren bis PTS(n). Rückwärts-Schritt nutzt Ringcache der letzten ~N dekodierten Frames, erst bei Cache-Miss wird neu gesucht.

### Videoausgabe
v1: swscale → RGBA, Upload via `TextureHandle::set`. Später (Optimierung): YUV-Planes + Shader (egui `PaintCallback`) bzw. D3D11VA-Hardwaredecoding.

## Meilensteine
1. **M1** – Gerüst, Fenster, Datei öffnen (Dialog/DnD/CLI), Wiedergabe (Videoclock), Play/Pause, Fehlermeldungen in der UI.
2. **M2** – Index-Scan, Framecount/Timecode/KEY-Badge, Frame-Stepping (Cache), Timeline mit Scrubbing + Keyframe-Markern, Unit-Tests.
3. **M3** – Audio (cpal), Master-Clock, Lautstärke/Mute.
4. **M4** – Loop I/O/L, PNG-Export, Neues Fenster (Ctrl+N), Fullscreen, Portable-Settings.
5. **M5** – Dark-Theme-Politur, Icon, README/CHANGELOG/LICENSE.
6. **M6** – GitHub Actions (CI + Release-ZIP mit SHA256), Performance-Tuning.

Jeder Meilenstein: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo build --release`, Commit (Conventional Commits).

## Status

M1–M6 sind umgesetzt (siehe `CHANGELOG.md` und Git-Historie). Offene Optimierungen für spätere Versionen:
YUV-Upload per Shader statt RGBA, D3D11VA-Hardwaredecoding, lückenloser Loop-Sprung über den Frame-Cache.

## Nicht im Scope (v1)
Playlists, Untertitel, Streaming, Filter, Installer, Auto-Updater, synchronisierte Wiedergabe zwischen Instanzen.
