# Changelog

Alle nennenswerten Änderungen an FrameScope. Das Format folgt
[Keep a Changelog](https://keepachangelog.com/de/1.1.0/), die Versionierung [SemVer](https://semver.org/lang/de/).

## [Unreleased]

## [0.7.0] - 2026-10-26

### Hinzugefügt
- **Codec-Informationen** (Taste `D` oder Info-Button): Container, Codec, Profil/Level, Auflösung, Bildfolge, Pixelformat, Farbraum, Datenrate und Tonspur; im Vergleich für A und B nebeneinander.
- **Bildrate mit p/i** in der Steuerleiste („25 fps · p“); interlaced wird aus der Container-Angabe oder der Frame-Markierung erkannt.

### Geändert
- Die Dateinamen-Marken im Vergleich (A/B) werden zusammen mit den Bedienelementen ausgeblendet.
- Safe-Area-Beschriftungen liegen gestaffelt untereinander und überdecken sich nicht mehr.

## [0.6.0] - 2026-10-25

### Hinzugefügt
- **Safe-Area-Ebenen (Title-Safe).** Über den Button oben rechts lassen sich beliebig viele Ebenen über das Bild legen: TV Action-Safe 93 % und Title-Safe 90 % (EBU R 95, wie bei SRG/ARD/ZDF) sowie die YouTube-Vorgaben für horizontale (16:9), quadratische (1:1) und vertikale (9:16) Video-Ads. Die Linien haben eine dunkle Unterlage und sind auf hellem wie dunklem Bild sichtbar; `Z` blendet die Auswahl ein und aus, sie wird in `framescope.ini` gespeichert.
- **macOS (experimentell, ungetestet):** App-Bundle für Apple Silicon und Intel, ad hoc signiert, mit selbst gebautem LGPL-FFmpeg (eigener Workflow `release-macos.yml`, Pre-Release über Tags `macos-v…`). Native Fenster-Ampel statt eigener Fenstersteuerung, Einstellungen neben dem `.app`; „Fenster anordnen“ nur unter Windows.

### Geändert
- Der Verlauf unter den Bedienelementen ist höher und deckender, damit Frame-Zähler und Timecode auch auf weißem Bild lesbar bleiben.

## [0.5.0] - 2026-10-13

### Hinzugefügt
- **Datei auf eine Fensterhälfte ziehen.** Läuft schon ein Video, wird eine hineingezogene Datei links zu Video A und rechts zu Video B; beim Ziehen markiert das Fenster das Ziel. Ohne laufenden Vergleich wird beim Ablegen links das bisherige Video zu B. Die Zeigerposition wird unter Windows direkt abgefragt, weil der Explorer beim Ziehen keine Mausereignisse an das Fenster sendet.

## [0.4.3] - 2026-10-13

### Behoben
- **Stocken bei gehaltener Pfeiltaste.** Beim Weiterschalten über einen Keyframe startete jeder Tastendruck den Seek neu, sodass bei schneller Tastenwiederholung kein Bild mehr ankam, bis die Taste losgelassen wurde. Ein laufendes Ziel wird jetzt verlängert statt neu gesucht, und wenige Frames werden weiterdekodiert statt per Seek angesprungen (4K: vorher ~0,5 s Stillstand an jedem Keyframe, jetzt keiner).

### Hinzugefügt
- Entwickler-Flag `--step-bench <Datei> [Rate]`: simuliert gehaltene Taste „Frame vor“ und meldet Stillstände.

## [0.4.2] - 2026-10-12

### Geändert
- Die Tastenkürzel-Übersicht (F1) zeigt die Versionsnummer.
- Eckige Fensterecken: die von Windows 11 abgerundeten Ecken sind abgeschaltet, damit das Video vollständig sichtbar ist.
- Kein Fensterrand mehr, wenn die Bedienelemente ausgeblendet sind (und nie im Vollbild).
- Der Bereich um das Video (Letterbox) ist jetzt schwarz.

## [0.4.1] - 2026-10-09

### Behoben
- **Sync: Start nach Pause.** Folgefenster sprangen beim Start jedes Mal per Seek vor und liefen erst nach 1–2 Sekunden synchron. Jetzt startet ein Fenster, das schon auf dem richtigen Frame steht, sofort ohne Seek, und das führende Fenster kündigt Play ca. 250 ms im Voraus an, damit alle gemeinsam loslaufen (gemessen: 4 Fenster direkt nach Play auf demselben Frame, danach höchstens 1–2 Frames Abstand statt bis zu ~40 Frames).
- **Ton nach Video-Nachholen.** Wurde das Video ohne Seek auf einen Frame nachgeholt (Decoder läuft vorwärts oder Frame aus dem Cache), blieb der Ton an der alten Stelle; beim Start sprang die Position zurück. Der Ton wird jetzt bei Abweichung über 0,08 s neu ausgerichtet.
- **Audio-Clock nach einer Pause.** Direkt nach dem Fortsetzen lieferte die Clock kurz einen veralteten Wert (Pausenposition plus Pausendauer), wodurch das Video Frames übersprang. Das Video wartet nach dem Start nun, bis der Ton wirklich läuft.
- Folgefenster erklären sich beim geplanten Start nicht mehr selbst zum Führenden.
- Drift-Korrektur im Sync reagiert schneller (Abkühlzeit 0,8 s statt 1,2 s).

### Hinzugefügt
- Rote Warnanzeige bei Frame-Ausfällen: unterscheidet „verworfen“ (Anzeige zu langsam) von „Decoder zu langsam“.
- Diagnose-Schalter per Umgebungsvariable (`FRAMESCOPE_NO_VSYNC`, `FRAMESCOPE_DECODE_THREADS`, `FRAMESCOPE_QUEUE`, `FRAMESCOPE_UI_PRIO`); erweiterte Statistik von `FRAMESCOPE_BENCH`.

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

[Unreleased]: https://github.com/supaeasy/framescope/compare/v0.4.1...HEAD
[0.4.1]: https://github.com/supaeasy/framescope/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/supaeasy/framescope/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/supaeasy/framescope/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/supaeasy/framescope/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/supaeasy/framescope/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/supaeasy/framescope/releases/tag/v0.1.0
