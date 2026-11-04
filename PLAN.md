# Kinetto: Plan

> Slides in, video out. A presentation and slideshow editor in pure Rust that
> anyone can use to make a polished video without any editing knowledge.

---

## 1. Name

### Pick: **Kinetto**
*kinetic* (motion) + *-etto* (small, friendly). It says "things move" without
saying "video editor", is easy to say in most languages, and has no meaning
that clashes with the product.

### Shortlist (crates.io checked 2026-11-03; all names are unclaimed there)

| Name | Idea behind it | GitHub repos with the name | Notes |
|---|---|---|---|
| **Kinetto** | kinetic + friendly suffix | 1 | Best fit: motion, short, nearly unused |
| **Slidra** | slide + -dra | 4 | Says "slides" plainly |
| **Showva** | show + nova | 33 | Bright and upbeat, a little playful |
| Ovida | Latin root *vid-* (see) | 32 | Elegant, but close to "Ovid" |
| Moviq | movie + q | 112 | Techy, harder to say |
| Glyde | glide | 178 | Sounds great, but crowded |

Rejected: *Lumio* (SMART Technologies already sells a lesson and
presentation product under it), *Slydo* (too close to Slido), *Frameo*,
*Lumeo* and *Kinova* (existing companies), *Motio* (~85k repos), and
*Sceno*, *Velora* and *Rillo* (already taken on crates.io).

**Before you commit:** search the trademark databases (USPTO TESS, EUIPO
eSearch, WIPO Global Brand DB) for "Kinetto" in classes 9 and 42, then
register kinetto.app or kinetto.com and the GitHub org. I couldn't finish the
domain and trademark checks.

---

## 2. Product principles: "simplest in the world"

1. **One screen, three zones.** Slide strip on the left, big canvas in the
   centre, one context panel on the right. No timeline is shown by default.
2. **The slide is the unit, not the frame.** Users never see keyframes or
   tracks. Each slide has a duration, an entrance style and an exit style.
   Timing is automatic.
3. **Smart defaults everywhere.** New text animates in nicely, transitions
   match the theme, and music ducks under voice. With zero settings the result
   should already look professional.
4. **One button: "Make Video".** It offers three presets: *YouTube 16:9*,
   *Shorts/Reels 9:16* and *Square 1:1*. Changing the aspect ratio re-flows
   the layouts automatically.
5. **Undo for everything and autosave always.** There is no "Save" anxiety.
6. **Instant.** Cold start under 300 ms, 60 fps preview, and export faster
   than real time on a normal laptop.
7. **Advanced mode hidden.** A "Timeline" toggle shows tracks for power users,
   but the default UI never needs it.

### Core features (v1)

- Themes: about 12 designer themes (fonts, palette, layouts, motion style).
- Slide layouts: Title, Title+Text, Image+Text, Full Image, Quote, List,
  Big Number, Video Clip.
- Elements: text, image, shape, icon, video clip, chart (simple bar/line/pie).
- Motion presets per element (Fade, Rise, Pop, Typewriter, Ken Burns for
  photos). They are named in plain words, not as easing curves.
- Transitions per slide (Cut, Fade, Slide, Zoom, Morph between matching
  elements).
- Audio: a built-in royalty-free music library, voice-over recording per
  slide, and auto-ducking.
- **AI helpers (offline, optional):**
  - *Narrate*: text to speech per slide with Kokoro, so slide duration fits
    the voice automatically.
  - *Auto-captions*: speech to text with Whisper and word timestamps,
    rendered as styled captions.
  - *Outline to slides*: paste text and get it split into slides with layouts.
- Import: PowerPoint (.pptx) text, images and order; images; a folder of
  photos as a slideshow; Markdown.
- Export: MP4 (H.264 + AAC), GIF, PNG sequence, PDF of the slides. You can
  also present it live in full screen.

---

## 3. Lessons taken from FilmCraft

FilmCraft (storytold/filmcraft) is an open-source pure-Rust NLE. It has
already validated the hard architecture decisions, so we copy what works and
skip what we don't need.

| FilmCraft does | We do |
|---|---|
| egui + eframe, wgpu compositor, WGSL shaders | **Same.** Proven for media apps in Rust |
| A CPU reference compositor, with GPU output tested against it | **Same.** Golden-image tests keep preview and export identical |
| A strictly layered workspace enforced by `cargo xtask layers` | **Same.** UI crates can only appear in the top layers |
| Every UI action is an engine command with a stable id (650+) | **Same.** This one registry powers undo, the CLI, scripting and MCP |
| Integer `Tick(i64)` time at 254,016,000,000 ticks/s | **Same.** No float drift, and all frame and audio rates divide evenly |
| `Arc<Project>` copy-on-write snapshots | **Same.** Cheap undo and safe background rendering |
| JSON project, schema version, migration chain, atomic save, crash journal | **Same** |
| A prioritized decode worker pool; the UI never decodes | **Same**, but simpler: few video clips, mostly stills |
| Its own Rust implementations of ~15 codecs | **Skip.** Too much work. Use OS hardware encoders plus openh264 |
| Whisper and Kokoro on candle, behind feature flags | **Same** |
| Pro features (grading, scopes, AAF/OMF, 24-track mixer) | **Skip.** Not our audience |

---

## 4. Tech stack (best libraries, Nov 2026)

| Concern | Library | Why |
|---|---|---|
| Window + UI | **egui / eframe 0.37** | Fast to build, immediate mode suits a live canvas, runs natively and on the web, FilmCraft-proven |
| UI polish | Custom egui theme + `egui_extras`, `egui_dnd` for slide reorder | Custom styling makes it look like a consumer app, not a dev tool |
| GPU | **wgpu** (the version eframe pins) | Metal, Vulkan, DX12, WebGPU |
| 2D vector rendering | **vello** (0.10, pinned to the same wgpu as eframe) | GPU-compute 2D: shapes, gradients, text and clipping at 60 fps+ |
| Text layout | **parley** (+ fontique, harfrust, skrifa) | Rich text, bidi, fallback; the Linebender stack that matches vello |
| Images | `image` + `fast_image_resize` + `resvg` (SVG/icons) | Standard and fast |
| Animation | Own small easing/spring module (`keyframe` crate optional) | Presets compile to curves the user never sees |
| Video encode | **Platform encoders** (VideoToolbox, Media Foundation, VA-API/NVENC) + **openh264** fallback | Small binary, no FFmpeg licensing, hardware speed |
| Mux | `mp4` / `muxide` (MP4 writer) | Pure Rust |
| Video clip decode | openh264 decode + platform decoders | Enough for imported phone clips |
| Audio decode | **symphonia** | MP3, FLAC, AAC, WAV, Vorbis in pure Rust |
| Audio encode | `fdk-aac` (optional) or platform AAC, with `opus` for WebM | AAC is needed for MP4 |
| Audio playback / record | **cpal** | Cross-platform; its clock drives preview sync |
| Resampling | `rubato` | High-quality sample rate conversion |
| Speech to text | **whisper-rs** (whisper.cpp) or candle-whisper | Word timestamps for captions |
| Text to speech | **any-tts / Kokoro on candle** | Offline, natural voice, no espeak dependency |
| PPTX import | `zip` + `quick-xml` | Read slide XML, text, images and order |
| PDF export | `krilla` | Modern pure-Rust PDF writer |
| GIF export | `gifski` | Best-quality GIFs |
| Serialization | `serde`, `serde_json` | Project file |
| Concurrency | `rayon` (CPU), `crossbeam-channel` (render jobs) | Simple and fast |
| Errors / logs | `thiserror`, `anyhow` (apps only), `tracing` | Standard |
| File dialogs | `rfd` | Native dialogs |
| Automation | `rmcp` (MCP server), `clap` (CLI) | Scriptable, AI-drivable |
| Tests | `egui_kittest`, `insta`, `proptest`, golden PNGs | UI, snapshot and property tests |
| Packaging | `cargo-dist` + `cargo-packager` | MSI, DMG, AppImage, Flatpak, deb |

**Version-lock rule:** eframe, wgpu and vello must agree on a single wgpu
version. Pin all three in the workspace `Cargo.toml` and upgrade them together.

**Alternative UI choice:** iced 0.14 (Elm architecture, `shader` widget for
custom wgpu) is the main contender. Choose egui for speed of development and
FilmCraft precedent.

---

## 5. Architecture

### Workspace layers (edges only point down, enforced by `cargo xtask layers`)

```
L0  kn-time      Tick, FrameRate, Duration, easing curves
    kn-geom      Point, Rect, Affine, Color (linear f32, premultiplied)
L1  kn-model     Project → Slides → Elements, Theme, Motion, Audio; serde
    kn-text      parley wrapper, font registry, bundled OFL fonts
    kn-media     image/video/audio sources, byte-budgeted frame cache
L2  kn-layout    theme + layout → element boxes; aspect-ratio re-flow
    kn-motion    presets → per-element transform/opacity curves
    kn-timeline  slides → absolute timeline (auto durations, transitions)
    kn-ai        whisper, kokoro, outline→slides (feature-gated)
    kn-import    pptx, markdown, image folder
L3  kn-render    Scene graph at time t → vello Scene; CPU reference path
    kn-audio     mixer, ducking, loudness normalize (EBU R128 −14 LUFS)
    kn-export    render → encode → mux job with progress and cancel
L4  kn-engine    Session, command registry, undo/redo, autosave, jobs
L5  kn-ui        egui app: strip, canvas, inspector, themes, onboarding
    kn-platform  OS encoders FFI (the only crate allowed to use `unsafe`)
    kn-automation MCP + JSON control channel
L6  apps/kinetto      desktop binary
    apps/kinetto-cli  render / export / exec / mcp
    apps/kinetto-web  wasm32 (later)
```

### One render function, two consumers

```
fn render_frame(project: &Project, t: Tick, size: Size) -> vello::Scene
```

- **Preview:** the canvas calls `render_frame` every frame, and vello draws it
  to an offscreen wgpu texture shown in egui. It is identical to export by
  construction (WYSIWYG).
- **Export:** a worker thread calls `render_frame` for each frame at full
  resolution. Readback goes to NV12 on the GPU (a compute shader does the
  color conversion), then to the encoder. This uses double-buffered readback,
  so the GPU and encoder overlap.
- **Golden tests:** render known projects at fixed times and compare the PNGs
  with a tolerance, on CI for all three OSes.

### Command registry (the backbone)

```rust
pub trait Command: Send + Sync {
    const ID: &'static str;            // "slide.add", "element.setText", ...
    type Args: Serialize + DeserializeOwned + JsonSchema;
    fn apply(&self, p: &Project, a: &Self::Args) -> Result<Project, CmdError>;
}
```

- The UI never mutates state directly; it dispatches commands.
- Undo stack = `Vec<Arc<Project>>` snapshots with structural sharing (`im`
  crate or `Arc` per slide). Coalesce rapid edits such as typing and dragging.
- The same registry is exposed via the CLI (`kinetto-cli exec slide.add …`),
  JSONL scripts and the MCP server, so an AI assistant can build a whole video.

### Project file `.kinetto`

- A zip bundle: `project.json` + `media/` (copied in, so projects are
  portable and you can send them to a friend).
- `{"format":"kinetto.project","schema_version":N,...}` with an ordered
  migration chain. Refuse to open newer files, and back up before upgrading.
- Atomic save: write to a temp file, fsync, rename. Autosave 1 s after each
  edit to a recovery journal.

### Performance budget

| Metric | Target |
|---|---|
| Cold start to usable | < 300 ms |
| Canvas interaction | 120 fps capable, never below 60 |
| Slide thumbnail refresh | < 16 ms (rendered by the same vello path at small size) |
| 1080p30 export, 3-min video | < 60 s with hardware encode |
| Binary size (without AI models) | < 40 MB |
| AI models | Downloaded on first use, never bundled |

---

## 6. UX flow (zero knowledge)

1. **Start screen:** "What are you making?" offers Tutorial, Product promo,
   Photo story, Lesson, Social post and Blank. Each choice picks a theme and an
   aspect ratio.
2. **Add content:** type on the canvas, or drag images in. You can also
   "Paste your text" and Outline→Slides builds the deck.
3. **Make it move:** an "Animate all" button applies the theme's motion. Users
   can change the motion of one element from a list of five named presets
   with live hover previews.
4. **Add sound:** pick music by mood (Calm / Upbeat / Epic). Add voice by
   recording or with "Read it for me" (TTS). Captions are one toggle.
5. **Make Video:** pick a preset, see a progress bar with a live frame
   preview, and then choose "Open folder" or "Share".

Onboarding: a 3-step coach-mark tour on first launch only, plus an empty-state
canvas that says *"Click to type, or drop pictures here."*

---

## 7. Roadmap

| Phase | Weeks | Deliverable |
|---|---|---|
| **0. Skeleton** | 1–2 | Workspace and layer lint, CI (Win/macOS/Linux), eframe window, vello canvas in egui, wgpu version lock |
| **1. Editor core** | 3–6 | Model, commands, undo, text/image/shape elements, slide strip, inspector, `.kinetto` save/load with autosave |
| **2. Motion** | 7–9 | Timeline builder, motion presets, transitions, play preview with cpal clock |
| **3. Export** | 10–12 | Frame loop, GPU NV12 readback, VideoToolbox/MF/VA-API + openh264, MP4 mux, AAC, progress/cancel; golden tests |
| **4. Polish = "simplest"** | 13–15 | Themes, templates, start screen, aspect re-flow, music library, ducking, onboarding |
| **5. AI helpers** | 16–18 | Kokoro narrate, Whisper captions, outline→slides, model downloader |
| **6. Import & ship** | 19–21 | PPTX/Markdown import, GIF/PDF export, present mode, installers, signing, auto-update |
| **7. Later** | — | Web build (wasm + WebGPU), MCP/CLI automation docs, plugin themes, team templates |

### Definition of done for v1

- A first-time user makes a 60-second video in under 5 minutes without help
  (test this with 5 real non-technical people).
- Preview and export pass golden tests on all three OSes.
- No data loss: kill the process mid-edit, and the work is restored on
  relaunch.

---

## 8. Quality rules

- `#![forbid(unsafe_code)]` everywhere except `kn-platform`.
- `clippy -D warnings`, `cargo deny` (licenses: forbid GPL deps in the core),
  `cargo audit` in CI.
- Every command has a property test (apply then undo restores the project
  exactly).
- Accessibility: egui AccessKit is enabled, everything is reachable by
  keyboard, and scaling is respected.
- i18n from day one (`fluent`); include RTL text tests since parley supports
  bidi.

---

## Sources

- FilmCraft: [GitHub](https://raw.githubusercontent.com/storytold/filmcraft/main/README.md), [Snap Store](https://snapcraft.io/filmcraft), [AlternativeTo](https://alternativeto.net/software/filmcraft/)
- Rust GUI comparison: [pistack 2026 comparison](https://www.pistack.xyz/posts/2026-08-25-rust-gui-frameworks-egui-iced-slint-comparison/), [GPUI comparison doc](https://docs.rs/gpui-ce/0.3.0/gpui/_documentation/comparison/index.html), [users.rust-lang.org thread](https://users.rust-lang.org/t/which-gui-framwork-should-i-choose/137581)
- iced: [0.14.0 release](https://github.com/iced-rs/iced/releases/tag/0.14.0), [shader widget](https://docs.rs/crate/iced_widget/0.14.2/source/src/shader.rs)
- vello: [crates.io](https://crates.io/api/v1/crates/vello), [vello_hybrid](https://lib.rs/crates/vello_hybrid)
- Text: [parley](https://github.com/linebender/parley), [cosmic-text](https://github.com/hecrj/cosmic-text)
- Media: [State of media processing in Rust 2026](https://dev.to/yeauty/the-state-of-media-processing-in-rust-2026-what-each-crate-actually-covers-2k2c), [ffmpeg-sidecar](https://docs.rs/crate/ffmpeg-sidecar/latest), [ez-ffmpeg](https://docs.rs/crate/ez-ffmpeg/latest/source/src/core/writer/mod.rs), [video-rs](https://github.com/oddity-ai/video-rs)
- AI: [any-tts](https://github.com/TM9657/any-tts), [kokoro-tts](https://docs.rs/crate/kokoro-tts/latest/source), [whisper-rs](https://openapps.pro/packages/whisper-rs), [oxiwhisper](https://docs.rs/oxiwhisper)
