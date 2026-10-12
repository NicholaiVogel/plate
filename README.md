# plate

A color-managed media player for VFX review on Linux. Plays movies, image
sequences and DCPs through OpenColorIO, with frame-accurate scrubbing,
channel isolation, A/B compare, pixel probe, and Frame.io-style comments
with drawn annotations.

Written in Rust with [gpui](https://gpui.rs). OpenColorIO and OpenImageIO
sit behind a small C++ shim; FFmpeg is used through its C API.

## What it opens

| Kind | Formats | Decoder |
| --- | --- | --- |
| Movies | MOV/QuickTime (ProRes, DNx, H.264, HEVC, …), MP4, MXF, MKV, WebM, AVI, … | FFmpeg |
| Sequences & stills | EXR (any compression, incl. DWAA/DWAB), DPX, Cineon, TIFF, PNG, JPEG, TGA, PSD, JPEG 2000, … | `exr` crate, `image` crate, OpenImageIO fallback |
| DCP | Unencrypted Interop and SMPTE packages, multi-reel, with sound | FFmpeg (JPEG 2000 X'Y'Z' → display-linear Rec.709) |

Opening one frame of a numbered sequence opens the whole sequence. Opening a
folder fills the bin with its movies, grouped sequences, stills, DCPs and
subfolders. Encrypted DCPs (KDM) are not supported.

## Color

- OCIO config: automatic (`$OCIO` if set, else OCIO's built-in ACES studio
  config), the built-in config, `$OCIO`, or any `config.ocio` file. Change it in
  Settings.
- The input colorspace and display/view are remembered **per format**:
  container plus codec or sample type (`mov/prores`, `mp4/h264`, `exr/half`,
  `dpx/10bit`, …). Pick a colorspace for one ProRes file and every ProRes file
  opens that way until you change it. Memory is kept per OCIO config.
- First-time defaults: float EXR → `scene_linear` with the config's default
  view; video and 8/16-bit stills → sRGB-encoded with an un-tone-mapped view
  (so they look like any other player); DPX → ADX10; DCP → linear Rec.709.
- The view transform is baked from the exact OCIO processor into a 65³ 3D LUT
  with a half-float-indexed shaper and applied with tetrahedral interpolation.
  That keeps the ACES 2.0 output transforms at playback speed (≈5 ms per 2K
  frame); error against the exact transform is well under one 8-bit code value
  except at ACES 2.0's hard clip to display white.
- Viewer exposure (stops, scene-linear) and gamma (display) are part of the
  baked transform.

## Cache

Decoded frames live in RAM (a quarter of physical memory by default, set in
Settings). Short clips are cached whole, so scrubbing is instant once loaded;
8-bit sources are cached as 8-bit.

An optional disk tier (Settings → Disk cache) takes frames evicted from RAM and
reads them back instead of decoding again, which helps with long-GOP movies,
EXR and DCP. Point it at a fast drive. Each session gets its own folder under
the chosen location, deleted when plate closes; folders left by a crash are
removed on the next start. Nothing persists between sessions.

## Bin

Opening a file lists its folder; opening a folder lists it directly. The bin
follows the disk live: new renders, deleted versions and copied-in files show
up as they happen, and a sequence that grows while it's open (a render in
progress) extends in place without losing the frame, cache or notes.

## Review

- Comments panel: write a note at the current frame; it is pinned to that frame
  and shown as a marker on the timeline. Mark done, delete, jump with `[` `]`.
- Draw with `D`: pen, arrow, line, rectangle, ellipse, text, eraser, seven
  colors, three stroke widths. Marks attach to the comment you post. Shift
  snaps lines to 45° and boxes to squares.
- Notes are saved as a JSON sidecar next to the media (`clip.mov.plate.json`,
  `shot_v001.exr.plate.json` for a sequence, `plate-notes.json` inside a DCP).
  If the folder is read-only they go to `~/.local/share/plate/notes/`.
- `Ctrl S` saves the displayed frame at full resolution with the view
  transform, channel and annotations burned in (PNG, JPEG or TIFF).

`Ctrl +` / `Ctrl −` / `Ctrl 0` scale the whole UI (remembered across restarts).
Press `?` in the app for every shortcut.

## Build

Arch packages: `ffmpeg opencolorio openimageio openexr` (plus a Rust toolchain).

```sh
cargo build --release -p plate
target/release/plate path/to/shot_v001.1001.exr
```

`.cargo/config.toml` pins the system C/C++ toolchain so a conda base env on
`PATH` doesn't leak its compiler and headers into the build.
`vendor/xattr` patches an old dependency of gpui that doesn't compile against
current `libc`.

## Install and default app

```sh
scripts/install.sh                 # ~/.local/bin/plate, desktop entry, icon
scripts/install.sh --set-default   # also the default for video, EXR, DPX, MXF
```

Folders keep their default handler (drop a folder on the window or use `Ctrl Shift O` to open it as a bin).

## Layout

```
crates/plate-native   C++ shim (cxx) over OpenColorIO and OpenImageIO
crates/plate-core     decoding, sequence detection, DCP, audio, frame cache,
                      background loaders, display LUT, prefs, notes, still baking
crates/plate          the gpui app
scripts/              test media generator, installer
packaging/            desktop entry and icon
```

`scripts/make-testmedia.sh` generates synthetic H.264, ProRes 4444, EXR
(ZIP16 and DWAA), DPX, 16-bit PNG and a two-reel DCP into `testmedia/`, plus a
real SMPTE DCP when DCP-o-matic is installed. The `plate-core` examples
`probe` and `bench` decode those through the full display pipeline.

Development hooks: `PLATE_DEMO=<steps>` puts the app into a given UI state
after launch (see `crates/plate/src/demo.rs`), `PLATE_STATS=1` logs the
presented frame rate during playback, and `PLATE_APP_ID` overrides the Wayland
app id.

## License

MIT, see [LICENSE](LICENSE). Bundled fonts (Inter Tight, DM Mono) are under the
SIL Open Font License; their license texts sit next to them in
`crates/plate/assets/fonts/`. `crates/plate/src/text_input.rs` is adapted from
gpui's Apache-2.0 input example.
