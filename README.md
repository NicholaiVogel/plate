# plate

A color-managed media player for VFX review on Linux and macOS. Plays movies, image
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
- Viewer exposure (stops, scene-linear) and gamma (display) are sliders in the
  top bar: drag, scroll to nudge, double-click to reset. Both are part of the
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

The sidebar is a tree, like a code editor's explorer. Opening a file roots it
at the file's folder (or reveals the file, if it's already under the root);
opening a folder roots it there. Folders expand in place and are scanned when
first opened; double-click a folder to make it the root, or step the root up
with the arrow. Sequences, DCPs and version stacks appear at every level, and
↑/↓ steps through clips in tree order.

The tree follows the disk live: the root and every expanded folder are
watched, so new renders, deleted versions and copied-in files appear as they
happen, and a sequence that grows while it's open (a render in progress)
extends in place without losing the frame, cache or notes.

## Review list and compare

Opening several clips at once (`plate preview a b c`, dropping files, or a
multi-select in Open) puts them in the **Review** list at the top of the
sidebar, whatever folders they're in. The first opens; ↑/↓ steps through the
list while the open clip is on it. Each row shows the clip and its folder.
Hover any tree row for **+** (add to Review) or **A/B** (compare as B); hover a
Review row to compare or remove it. The list is kept between sessions.

Compare works across folders: the A/B modal lists Review clips first, then the
tree, and **Choose file…** takes anything on disk. The B label shows its
folder when it differs from A's.

## Versions and layers

- Versions: files that differ only by a standalone version token
  (`shot_v001.mov` … `shot_v009.mov`, `comp.v12.####.exr`) stack into one bin
  row showing the latest, expandable to the rest. `v1rough`-style milestone
  names aren't mistaken for versions. `Alt ↑/↓` steps older/newer on the same
  frame, keeping zoom, pan, range, layer and playback; `V` lists versions with
  their age; `Shift V` wipes against the previous version. A new version
  landing on disk gets a toast.
- Multi-layer EXR: layers are found from channel names (`R G B A`,
  `layer.R`, Nuke's `layer.red`, Blender's `ViewLayer.Pass.R`, single-channel
  and XYZ data passes) across all parts of multi-part files, beauty first. A
  Layer control appears in the top bar; `Alt ←/→` steps layers. Only the
  chosen layer's channels are read, each layer caches separately, and the
  choice carries over to other files and versions that have the layer.

## Guides and captions

- Guides (grid button, `'` to show/hide): rule of thirds, center cross, action
  safe (93%) and title safe (90%), an aspect mask (2.39, 2.35, 2.00, 1.85,
  16:9, 4:3, 1:1, 4:5, 9:16) at 50/75/100%, and grids up to 12×12. Everything
  is measured inside the mask, so thirds and safe areas follow the extraction.
  Remembered between sessions.
- Captions (CC button, `S` to show/hide): SRT and WebVTT. A sidecar named like
  the clip (`clip.srt`, `clip.en.vtt`, `shot_v001.srt` for a sequence) loads
  automatically; others can be loaded by hand. Size, position and background
  box are adjustable, plus a frame offset for files that don't start at zero.

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

On macOS, use `Cmd` in place of `Ctrl` for file, clipboard and undo shortcuts.
`Ctrl +` / `Ctrl −` / `Ctrl 0` scale the whole UI (remembered across restarts).
Press `?` in the app for every shortcut.

## Command line

```sh
plate preview shot_v001.1001.exr --frame 1012     # show it in the open plate (or start one) and return
plate preview v003.mov --compare v002.mov          # A/B, any two folders
plate preview a/sh010.mov b/sh020.mov c/sh030.mov  # Review list: several shots, any folders
plate preview --add d/sh040.mov                    # append to the Review list
plate info review.mp4 [--json]                     # format, range, fps, color plate would use, comments
plate still shot_v001.1001.exr --frame 1012 -o f.png [--exposure 1] [--annotations]
plate clip.mov                                     # GUI; hands off to an open plate if there is one
plate clip.mov --new                               # force a separate window
```

`plate preview` is meant for scripts and agents: it never blocks. With a plate
window open, the media is handed over a Unix socket in `$XDG_RUNTIME_DIR` and
the window is raised; otherwise plate starts detached (log in
`~/.local/state/plate/plate.log`). Frames are given as numbered in plate
(1001-based for comp sequences, 0-based for movies). Errors such as a missing
file or a frame outside the range come back on stderr with exit code 1, and
nothing on screen changes. `plate still` renders with the same per-format
color choice the GUI remembers.

## Build

### macOS

Install a Rust toolchain and Apple's Command Line Tools (`xcode-select --install`).
GPUI compiles its Metal shaders at startup, so the full Xcode app is not required.

If your Homebrew installation provides these packages:

```sh
brew install pkg-config ffmpeg opencolorio openimageio
```

Alternatively, install compatible native libraries into `target/macos-deps/`.
This works on Intel and Apple Silicon and avoids changing system packages:

```sh
scripts/setup-macos.sh
export PATH="$PWD/target/macos-deps/bin:$PATH"
cargo build -p plate
target/debug/plate path/to/shot_v001.1001.exr
cargo test --workspace
```

The setup script downloads micromamba and uses conda-forge for OpenColorIO 2.5,
OpenImageIO 3.1 and FFmpeg 8. Cargo discovers this local prefix automatically;
an existing `PKG_CONFIG_PATH` takes precedence. Build against libraries for the
same architecture as your Rust toolchain.

Create a Finder-launchable app with its native libraries bundled:

```sh
scripts/build-macos.sh             # target/Plate.app, release build
scripts/build-macos.sh --debug     # faster development build
open target/Plate.app
```

The bundle includes a native icon at all macOS resolutions. Its source is
`packaging/plate.svg`; regenerate the PNG and `.icns` with
`scripts/build-icon.sh` (requires `rsvg-convert` from librsvg).

The app is ad-hoc signed for local use. Distribution to other Macs requires
Developer ID signing and notarization. Finder's Open With, document-opening
events, native file dialogs, and Command-key shortcuts are supported.
The native traffic lights sit inside Plate's header; drag the file-title area
to move the window, or double-click it for the system titlebar action.

### Linux

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

On macOS, the same installer creates `~/Applications/Plate.app` and a
`~/.local/bin/plate` launcher. Use `scripts/install.sh --debug` to install a
development build. To change a default handler, use Finder's Get Info →
Open with → Plate → Change All; `--set-default` prints these steps on macOS.

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

On macOS, `PLATE_CHECK_SCRUB=1 target/debug/plate path/to/movie.mov` runs a
native interaction check and quits. It verifies header control positions,
timeline dragging, seeks beyond both window edges, and mouse release outside
the window. The input must contain more than one frame.

## License

MIT, see [LICENSE](LICENSE). Bundled fonts (Source Sans 3, Source Code Pro)
are under the SIL Open Font License; their license texts sit next to them in
`crates/plate/assets/fonts/`. The interface uses SF Pro and SF Mono instead
when they're installed (Settings → Interface font); those aren't bundled. `crates/plate/src/text_input.rs` is adapted from
gpui's Apache-2.0 input example.
