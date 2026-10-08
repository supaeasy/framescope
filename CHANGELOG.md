# Changelog

Alle nennenswerten Änderungen an FrameScope. Das Format folgt
[Keep a Changelog](https://keepachangelog.com/de/1.1.0/), die Versionierung [SemVer](https://semver.org/lang/de/).

## [Unreleased]

## [0.4.0] - 2026-10-09

### Hinzugefügt
- **Rahmenloses Fenster** ohne Titelleiste: Fenstersteuerung (minimieren, maximieren, schließen) oben rechts, Verschieben per Ziehen im Bild, Größenänderung an den Rändern.
- **Immer im Vordergrund** (Pin-Button, `T`).
- **Fenster anordnen** (Button, `G`): alle FrameScope-Fenster gleich groß, lückenlos im Raster (2–3 übereinander, 4 im 2×2-Raster, …).
- **Klick ins Bild** startet/pausiert die Wiedergabe.
- **Originalgröße** (Button, `1`): Fenster auf die Videoauflösung setzen, pixelgenau 1:1.

## [0.3.0] - 2026-10-08

### Geändert
- **Neues Design „Nocturne“** nach den Mockups: blaugraue Oberfläche mit violettem Akzent (nur für Abspielkopf, Loop-Bereich und KEY), Schrift Inter mit tabellarischen Ziffern, Phosphor-Icons, durchgehende Control-Leiste mit Verlauf statt schwebender Karte, Frame-Schritte und Keyframe-Sprünge als Buttons, Kamera- und Vollbild-Button in der Leiste, Info-Overlay (`H`) im neuen Stil. Infos zu Auflösung/Codec/Keyframes stehen jetzt im Tooltip des Frame-Zählers.

## [0.2.1] - 2026-10-08

### Geändert
- **Lizenz:** neue Lizenz „FrameScope License (No Sale Without Permission)“ – Nutzung, Änderung und Weitergabe bleiben frei, der **Verkauf** der Software oder abgeleiteter Versionen ist nur mit vorheriger schriftlicher Genehmigung erlaubt. Version 0.2.0 und älter bleibt unter der MIT-Lizenz.

## [0.2.0] - 2026-10-08

### Hinzugefügt
- A/B-Vergleich zweier Videos in einem Fenster (`Strg+B`, `framescope.exe A B`): Schieber, Nebeneinander, Überblenden (`C`); Video B läuft auf der Uhr von A.
- Synchronisierte Wiedergabe mehrerer Fenster (`Y`): Play/Pause, Scrubbing, Einzelbild, Keyframe- und Loop-Sprünge, frame-genau im Pausenzustand; Versatz-Abgleich (`Umschalt+Y`); `--sync` auf der Kommandozeile.

### Geändert
- Texturen werden nur noch bei neuem Frame hochgeladen (weniger CPU bei 4K); Infozeile wird in schmalen Fenstern ausgeblendet statt andere Bedienelemente zu überlappen.

### Hinzugefügt (Entwickler)
- Anzeige verworfener Frames, Entwickler-Flags `--version`, `--bench`, `--audio-selftest`.

## [0.1.0] - 2026-10-08

Erste Version.

### Hinzugefügt
- Wiedergabe gängiger Formate über FFmpeg (MP4/H.264, H.265, MKV, MOV, WebM, AVI) mit Play/Pause und Seek.
- Frame-Index (Hintergrund-Scan der Videopakete): Framecount, Timecode `hh:mm:ss:ff`, Keyframe-Erkennung; VFR-tauglich.
- Einzelbild-Stepping vor/zurück (Pfeiltasten) mit Frame-Cache, Sprung zwischen Keyframes (Umschalt+Pfeil).
- `KEY`-Badge am aktuellen Frame und Keyframe-Marker auf der Timeline.
- Audiowiedergabe mit Audio als Master-Clock, Lautstärke und Mute.
- Loop (`I`/`O`/`L`) mit sichtbarem Bereich auf der Timeline; ohne Marker wird das ganze Video wiederholt.
- PNG-Export des aktuellen Frames in Originalauflösung (`S`, Standardordner oder Dialog).
- Mehrere Instanzen (`Strg+N`), Öffnen per Drag & Drop, Dateidialog und Kommandozeilenargument.
- Vollbild, Info-Overlay (`H`), Tastenkürzel-Hilfe (`F1`).
- Portable: Einstellungen in `framescope.ini` neben der EXE, keine Registry, kein `%APPDATA%`.
- Dunkle, minimalistische Oberfläche mit ein-/ausblendbaren Controls, Fenster- und EXE-Icon.
- Korrekte Farbmatrix (BT.601/709/2020) bei der YUV→RGB-Umrechnung.

[Unreleased]: https://github.com/supaeasy/framescope/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/supaeasy/framescope/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/supaeasy/framescope/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/supaeasy/framescope/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/supaeasy/framescope/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/supaeasy/framescope/releases/tag/v0.1.0
