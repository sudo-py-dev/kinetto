# UI improvement plan — November 2026

Scope: the editor (`crates/kn-ui`), reviewed from 109 fresh screenshots of the
kittest harness (light, dark, 960 / 1280 / 1440 wide, all seven languages) and
a read of the UI code. It continues [audit.md](audit.md).

**Status:** fixed on branch `ui-quick-fixes`: #1–3 (one `clock` module),
#6 (the back chevron mirrors), #16, #22, the Music icon from #24, and #36.
Everything else is open.

Each item says where, what's wrong, why it matters and the fix. They are
ordered by severity: **P0** wrong result or broken task, **P1** RTL/i18n and
accessibility, **P2** hierarchy and flow, **P3** polish. Effort is S (under
an hour), M (half a day) or L (a day or more).

To re-make the screenshots:

```sh
mkdir -p out && for v in SCREENS CREATION_SCREENS AUDIO_SCREENS POWER_SCREENS LAYOUT_SCREENS \
  WELCOME_SCREENS SLIDE_SCREENS PICTURE_SCREENS MOTION_SCREENS TEMPLATE_SCREENS SCROLL_SCREENS; \
  do export KINETTO_$v=out; done; cargo test --release -p kn-ui --lib screens
```

---

## P0: wrong results

| # | Where | Problem | Fix | Effort |
|---|-------|---------|-----|--------|
| 1 | `export.rs:178`, `inspector.rs:451` | The clock takes minutes from the truncated value and seconds from the rounded one. A 59.6 s video shows **0:00**, and 2:59.6 shows **2:00**. | One shared `fmt::clock(secs)` that rounds once, then splits into minutes and seconds. Add a test for 59.6 → `1:00`. | S |
| 2 | `timeline.rs:116` | `{:04.1}` on `secs % 60` prints **0:60.0** for 59.96 s. | Round to tenths first, then split. Same helper as #1, in a tenths variant. | S |
| 3 | Three clock formats | The stage says `0:12 / 0:21`, the timeline `0:12.4 / 0:21.0` and the export dialog rounds differently, all on one screen (`docs/timeline.png`). | Use one helper everywhere. Show tenths only in the timeline toolbar, where precision matters, and the same `m:ss` elsewhere. | S |
| 4 | `adw::toggle_group` with text alignment (`inspector.rs:112`) | In RTL the group reverses its order, so **"Right" is drawn on the left and "Left" on the right** (`lang-he-item.png`). The labels also map to `TextAlign::Start/End`, which on a Hebrew text box means right/left, the opposite of the label. | Give `toggle_group` a `physical: bool` (or a separate `align_group`) that never reverses. Draw alignment as icons (three left/centre/right line glyphs) and resolve Start/End from the paragraph's own direction, not from the UI language. Do the same for "Align on slide" (`inspector.rs:167`). | M |
| 5 | Every `TextEdit` in Hebrew | Words show in **reverse order** while editing: the group title says "רגעים בחוץ", but the field under it shows "בחוץ רגעים", and the caption shows "יותר שקט בוקר" (`lang-he-quick.png`). Bullet dashes sit at the wrong end. This is the main thing a Hebrew user types into. | Give `TextEdit` a custom `layouter` that runs `unicode-bidi` per line and returns the galley in visual order, with a logical↔visual index map so the cursor and selection follow. Wrap it as `adw::text_field`. Until then, at least set right alignment and show a read-only preview line under multi-line fields. | L |

## P1: RTL and i18n

| # | Where | Problem | Fix | Effort |
|---|-------|---------|-----|--------|
| 6 | `inspector.rs:41` back button | `Icon::Left` isn't mirrored. In Hebrew the chevron sits right of "הגדרות השקף" and points left, into the text, which reads as "forward". | Add `Icon::Back` (directional), which `icons::paint` flips when `i18n::rtl()`. List which icons mirror (back, forward, transition, move-up/down arrows in lists, undo/redo per Material) and which never do (play, previous/next media, check, clock). | S |
| 7 | `adw::disclosure` (settings panel), quick-video groups | The egui ▸ triangles don't mirror and don't match the rounded chevron used in the quick dialog (`item-light.png` versus `quick.png`). | One disclosure header in `adw` using `icons::chevron`, rotating from "points to the start" to "points down". Its icon goes at the start side. | S |
| 8 | `finder.rs:296-299` | Finder rows paint icon, label and shortcut at fixed left/right offsets. In Hebrew the icon is on the left and the label is left-aligned. | Build the row with `adw::leading()` / `trailing()`, or mirror the x offsets when `rtl()`. Keep the shortcut hint in LTR (`Ctrl+Z`). | S |
| 9 | Items list (`inspector.rs:531-534`), timeline labels (`timeline.rs:215`), slide captions (`strip.rs:236`) | Icon, name, time and grip are painted with `LEFT_*` anchors, so in Hebrew the thumbnail caption is left-aligned under a right-aligned sidebar. The "No soundtrack" label (left) doesn't line up with "מוזיקה" (right) in the label column. | Add a `adw::row_anchor(rect) -> (start_x, end_x, Align2)` helper and use it in these painters. The timeline tracks stay LTR. | M |
| 10 | Transport bar (`stage.rs:170`) versus timeline toolbar (`timeline.rs:132`) | The timeline toolbar mirrors (play at the far right in Hebrew), while the stage transport keeps play at the left. Same controls, two orders. | Pick one rule: media controls stay LTR everywhere (the platform convention). Leave the toolbar's *group* order mirrored but keep the play/restart/time cluster inside it LTR. | S |
| 11 | Numbers | `format!("{:.1}")` and `{:.0}%` always print `1.5 s` and `60%`. French, German, Spanish and Russian expect `1,5 s` and `60 %` (with a no-break space). | `i18n::number(f64, decimals)` and `i18n::percent(f64)` using a small per-language table (decimal mark, percent spacing). Replace the ~12 call sites found by `grep ':\.1}\|:\.0}%'`. | S |
| 12 | Plurals (`start.rs:178`, `quick.rs:582`, `export.rs:198`, `import.rs:74-76`) | `n == 1 ? one : other`. Russian has worked around it with "Слайдов: {n}", but `"{n} things couldn't be brought in."` reads badly at n = 1, and Hebrew has a dual. | `trp!(one, other, n)` choosing a CLDR category per language (one/few/many/other for Russian, one/two/other for Hebrew). Keep the colon form as the fallback. | M |
| 13 | `import.rs:74` | The sentence is built by concatenation: `format!(" {}", …)` appended to "Made N slides". Word order can't change per language. | One template per case: `"{made}. {n} things couldn't be brought in."` | S |
| 14 | German text expansion | `adw::List::row` reserves 100–156 px for the control even when it's a 46 px switch, so "Dunkler Hintergrund / Für Abschnittswechsel" wraps into four lines. "Sonnenuntergang" breaks mid-word under its swatch (`lang-de-editor.png`). | Measure the control first (or take a `control_width` hint: switch 52, swatch 34, dropdown 156). Let look names shrink their font or truncate with a tooltip, and never break inside a word. | M |
| 15 | Untranslated accessible names | `"Video position"`, `"Timeline ruler"`, `"Find an action"`, `"Font"`, `"Photo preset"`, `"Entrance animation"`, `"Slide {n}"`, `"Open {name}"`, `"Add sample: …"`, `"Step {n}: …"` go to the screen reader in English. | Wrap them in `tr` / `trf!`, and add a test that every `WidgetInfo::labeled` literal is in the tables. | S |
| 16 | English copy says "on the right" | `"No music yet: … in the panel on the right"`, `"pick a look on the right"`. The translations already say "on the side", but in English the panel's side depends on the language. | Say "in the settings panel" in English too. | S |

## P1: accessibility and keyboard

| # | Where | Problem | Fix | Effort |
|---|-------|---------|-----|--------|
| 17 | Stage scrub bar (`stage.rs:191`) | It reports itself as a slider but has no keyboard handling: it can be focused, but arrows do nothing and there's no focus ring. | Reuse `adw::slider`'s key handling: ←/→ ±1 s, Shift ±5 s, Home/End. Draw the standard focus ring. | S |
| 18 | Focus rings | `adw::button` uses `accent × 0.6` (under 3:1 on the dark header), while every other widget uses the full `accent` through `focus_ring`. The slide strip, timeline blocks, items list rows, template cards and recent rows draw no focus at all (0 `has_focus` in `strip.rs`, `timeline.rs`, `inspector.rs`, `recent.rs`). | Use `focus_ring` everywhere and make the cards and rows focusable. Tab through the welcome page and the editor as a test. | M |
| 19 | Small text | 17 call sites draw text at 10.5–11.5 px (timeline status, ruler, voice labels, hints). At 100% scale that's below comfortable reading, and dim on grey. | Make 12 px the floor. Add `theme::type_scale` (12 / 13.5 / 14 / 15 / 19 / 28) and replace literal sizes with named steps. That also turns the ~20 distinct sizes into 6. | M |
| 20 | No UI zoom | There's no way to enlarge the interface (Ctrl + / Ctrl − / Ctrl 0) for low vision or 4K screens. | Bind them to `ctx.set_zoom_factor`, save it in settings, and add a "Text size" row to the main menu. Check the layouts at 150%. | S |
| 21 | Reduced motion | Only export reads `animation_time`. Panel slides, the disclosure, toasts and the welcome hover zoom ignore a reduced-motion setting. | Read the GNOME/Windows/macOS reduced-motion setting where available (or add a "Reduce motion" switch) and set `animation_time = 0`. Keep slide previews animated: they're the content. | S |

## P2: hierarchy and flow

| # | Where | Problem | Fix | Effort |
|---|-------|---------|-----|--------|
| 22 | Video tab (`power-settings-video-*.png`) | Its blue **Export…** button repeats the header's blue **Export**, so there are two primary actions on one screen. | Make it a normal (grey) button, or drop it. "This video" already sits next to the header button. | S |
| 23 | Sound tab: music placement | "Start at this slide", "Start at playhead" and "From video start" are three full-width grey buttons of equal weight, none showing which is current. Under them, a "Music starts" slider sets the same thing again (`power-settings-sound-light.png`). | One row, "Music starts", with a three-way toggle (This slide · Playhead · Video start) and the time as its subtitle. The slider becomes fine-tuning inside a disclosure. | M |
| 24 | Wrong or overloaded icons | The Music row in the timeline uses the **camera** icon (`timeline.rs:240`). The camera also stands for Export, Quick video, Create video and recent projects. "Restart video" uses **Undo** (`timeline.rs:138`), right next to the real Undo. "Show the … view" in the finder uses Transition. Duplicate and Copy share one icon. "Video shape: Square" uses a shape icon. | Add `Icon::Music` (note), `Icon::Restart` (bar plus back-triangle, like Previous), `Icon::Duplicate` (plus on sheet), `Icon::Layout` (two panes) and `Icon::Aspect` (frame). Keep the camera for Export only. | M |
| 25 | "Quick video" in the header bar | A flat text label with no icon between Undo/Redo and the empty space reads as a title, not a button (`editor.png`). | Give it the sparkle icon and a tooltip, or move it to the main menu and the finder (it's a start-of-project action). | S |
| 26 | Items list in the slide panel | A nested scroll at the bottom of a scrolling panel shows about 2 rows. With 47 items (`power-finder-typed-dark.png`) it's unusable. | Remove the inner scroll and let the panel scroll, or make it a collapsible group that shows 6–8 rows and a "Show all" option. | S |
| 27 | Slide sidebar transition chips | Each transition button takes about 50 px between thumbnails, so only 3.5 slides fit at 800 px tall. | Shrink it to a 24 px pill that sits on the gap and overlaps both thumbnails' margins, showing the label on hover or focus, or show the transition name inside the caption row. | M |
| 28 | Item panel header | "Text / Changes apply to the selected item" adds a heading and a sentence that say nothing new under "‹ Slide settings". | Drop the subtitle, and show the item's name or first words as the heading ("Text · PRODUCT LAUNCH / 01"). | S |
| 29 | Welcome page copy | The window subtitle "Make a video from slides", "Start with a story", its subline, and the Quick video card's eyebrow, title and subline make six taglines before the first template. | Drop the window subtitle on the welcome page and the "QUICK VIDEO" eyebrow (it repeats the button). Keep one hero line. | S |
| 30 | Destructive actions without undo | "Clear list" (`recent.rs:43`) clears history at once. The recovery banner's **Discard** deletes the safety copy at once. | Give Clear list an undo toast ("History cleared · Undo"). Make Discard ask first, with the consequence in the button ("Discard the unsaved video"). | S |
| 31 | Quick video photo group | The group's disclosure title and its name field show the same text, one right under the other (`quick.png`). | Make the title editable in place (click to rename), or show the field only while renaming. | S |
| 32 | Timeline empty rows | "Music" plus "No soundtrack" in the labels, and "No music yet…" in the track, say the same thing twice. The Voice row is cut off at the bottom at 800 px (`timeline.png`). | Keep only the track hint, and make it a link button that opens the Sound tab. Let the tracks panel get at least `RULER + SLIDE_ROW + 2 * ROW + MUSIC_ROW + VOICE_ROW` before the stage. | S |

## P3: polish

| # | Where | Problem | Fix | Effort |
|---|-------|---------|-----|--------|
| 33 | Window size | `main.rs` always opens at 1280×800. Size, position and maximised state aren't remembered. | Turn on eframe `persist_window` (the `persistence` feature), or save the size and position in `settings`. | S |
| 34 | Scrub-bar knob | The knob is always white with a light shadow. On the light theme it almost vanishes against `view`. | Give it a 1 px `border` stroke in the light theme, or fill it with `accent_bg`. | S |
| 35 | Selected look swatch | Selected swatches use a 2 px accent ring, the same as focus, so keyboard focus on another swatch looks like a second selection. | Selection: ring plus a check badge. Focus: the standard outer ring. | S |
| 36 | Inspector counter "1 / 4" | It repeats the sidebar count and the caption numbers. | Remove it, or replace it with "Slide 1". | S |
| 37 | Disabled icon buttons | They still show their tooltip and use 0.4 alpha. Some, like Redo in the dark header, are hard to tell from enabled ones. | Keep 0.4 alpha but add "(nothing to redo)" to the tooltip, so hovering explains it. | S |
| 38 | Context menu | Delete looks the same as everything else. | Use destructive colour for Delete's label and icon in menus. | S |

---

## Suggested order

1. **One quick PR (P0 #1–3, #6, #16, #22, #24-music icon, #36):** small, visible fixes, each with a test where there's logic.
2. **RTL PR (#4, #7–#10, #14, #15):** mirroring rules in one place (`adw` + `icons`), with Hebrew screenshots before and after.
3. **Locale PR (#11–#13):** number and plural helpers, plus a test over every language.
4. **Accessibility PR (#17–#21):** focus, type scale, zoom and reduced motion.
5. **Flow PR (#23, #25–#32):** the Sound tab, items list and sidebar density.
6. **Bidi text input (#5):** the one large piece. Spike the `unicode-bidi` layouter first, behind a setting, and test cursor movement and selection in mixed Hebrew and English.

## Checks after each step

- Make the screenshots in Hebrew and German at 960 and 1440 wide, in both themes.
- Tab through the welcome page, the quick dialog and the editor with the keyboard only: is focus always visible, and is the order logical?
- Stress the content: a 40-character video name, a slide with 50 items, a 59.6 s video, a 1-slide video.
- Run the app at 150% zoom.
