# Drittanbieter-Lizenzen

## FFmpeg (LGPL v2.1 oder neuer)

FrameScope verwendet die Bibliotheken von [FFmpeg](https://ffmpeg.org/) (`libavcodec`, `libavformat`, `libavutil`,
`libswscale`, `libswresample`; im Release-ZIP liegen nur diese fünf DLLs bei).

- Die DLLs im Release-ZIP stammen **unverändert** aus den **LGPL-Shared-Builds** von
  [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) (`win64-lgpl-shared`, FFmpeg 9.0). Diese Builds
  enthalten keine GPL- oder nonfree-Komponenten.
- FrameScope **linkt dynamisch** gegen diese DLLs. Du kannst sie durch eigene, kompatible FFmpeg-9.0-Builds
  ersetzen (gleiche Dateinamen/ABI), ohne FrameScope neu zu bauen.
- Der LGPL-Lizenztext liegt dem Release-ZIP als `licenses/FFmpeg-LICENSE.txt` bei
  (vollständiger Text: <https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html>).
- **Quellcode:** FFmpeg-Quellen: <https://ffmpeg.org/download.html> bzw. <https://github.com/FFmpeg/FFmpeg>;
  Build-Skripte und genaue Konfiguration der verwendeten Builds: <https://github.com/BtbN/FFmpeg-Builds>.
- Die Rust-Anbindung `ffmpeg-next` / `ffmpeg-sys-next` steht unter der WTFPL.

FFmpeg ist eine Marke von Fabrice Bellard, siehe <https://ffmpeg.org/legal.html>.

## Rust-Bibliotheken

| Crate | Lizenz |
|---|---|
| `eframe`, `egui` (und `egui_glow`, `epaint`, `emath`, `ecolor`) | MIT OR Apache-2.0 |
| `cpal` | Apache-2.0 |
| `ffmpeg-next`, `ffmpeg-sys-next` | WTFPL |
| `image` (PNG) | MIT OR Apache-2.0 |
| `egui-phosphor` (Phosphor-Icons, eingebettet) | MIT OR Apache-2.0 (Icons: MIT) |
| `rfd` | MIT |
| `crossbeam-channel` | MIT OR Apache-2.0 |
| `anyhow` | MIT OR Apache-2.0 |
| `embed-resource` (Build) | MIT |

Die transitiven Abhängigkeiten stehen unter vergleichbaren permissiven Lizenzen (MIT, Apache-2.0, BSD, Zlib, ISC,
Unicode). Eine vollständige Liste erzeugt z. B. `cargo install cargo-license && cargo license`.

## Schriften und Icons

- **Inter** (Medium, Latin-Teilmenge mit tabellarischen Ziffern, `assets/fonts/Inter-Medium-Tabular.otf`) –
  Copyright © 2016–2020 The Inter Project Authors, **SIL Open Font License 1.1**. Der Lizenztext liegt dem
  Release-ZIP als `licenses/Inter-OFL.txt` bei; das Ableitungsskript steht in `assets/fonts/make_font.py`.
- **Phosphor Icons** (über die Crate `egui-phosphor`) – MIT-Lizenz, <https://phosphoricons.com>.

egui bringt die Schriften *Hack*, *Ubuntu-Light*, *NotoEmoji* und *emoji-icon-font* mit
(Lizenzen: MIT/Bitstream Vera, Ubuntu Font Licence, SIL OFL 1.1, OFL/Apache-2.0).
