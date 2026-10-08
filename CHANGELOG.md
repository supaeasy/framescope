# Changelog

Alle nennenswerten Änderungen an FrameScope. Das Format folgt
[Keep a Changelog](https://keepachangelog.com/de/1.1.0/), die Versionierung [SemVer](https://semver.org/lang/de/).

## [Unreleased]

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

[Unreleased]: https://github.com/supaeasy/framescope/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/supaeasy/framescope/releases/tag/v0.1.0
