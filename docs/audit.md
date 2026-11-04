# UI/UX and code-quality audit — November 2026

Scope: the editor (`crates/kn-ui`), checked against screenshots of the real
window in light and dark themes (welcome, quick video, editor, timeline,
inspector, dialogs, banners). The other crates got a lint and test pass only.

Screenshots come from the kittest harness:

```sh
mkdir -p out
KINETTO_SCREENS=out KINETTO_CREATION_SCREENS=out KINETTO_AUDIO_SCREENS=out \
  cargo test -p kn-ui --lib screens
```

(`screens` matches every test in the `screens` module, so all three sets are made.)

## Fixed

| # | Area | Finding | Fix |
|---|------|---------|-----|
| 1 | Build | CI was red: `clippy::too_many_arguments` on `paint_waveform` (8 args). | Source span grouped into `WaveSpan`. `cargo clippy -D warnings` is clean. |
| 2 | UX | The problem dialog ignored Escape and Enter. Only the "Save changes?" dialog had Escape, wired up by hand in `lib.rs`. | `adw::alert` handles both for every dialog: Escape picks the first (safe) button, Enter the suggested one. The special case in `lib.rs` is gone. Test: `problem_dialog_closes_with_escape_or_enter_and_keeps_the_slide`. |
| 3 | UX consistency | The timeline's music volume used egui's stock slider (big hollow knob, built-in value box). Every other slider in the app is the Adwaita one. | Now uses `adw::slider` with a dim `60%` label beside it. |
| 4 | Accessibility | `adw::slider` declared itself a slider but ignored screen-reader SetValue/Increment/Decrement actions, and never reported its min, max or step. | It now handles those actions and fills in the AccessKit node. The existing volume test now exercises this path. |
| 5 | UX | "Items on this slide" showed the word `timed` on every row of every template. It carried no information. | Each row shows when the item appears (`0.6 s`). |
| 6 | UX | Every shape was listed as "Shape" in the items list and the timeline, so you couldn't tell them apart. | Rows use the shape's name ("Rectangle", "Star", …). |
| 7 | Visual | Quick video dialog: the "1. Your story" and "2. Make it yours" titles were 4 px out of line. | `quick::heading` uses the same top space as `adw::group`. |
| 8 | Layout | Long toasts (e.g. "No speakers or headphones were found…") ran on one line and could be wider than the window. Long banner text could push its buttons off-screen. | Toasts wrap at a width based on the window size. Banners keep their buttons on the right and wrap the message beside them. |
| 9 | Keyboard | F5 could start presenting while a dialog was open. | F5 is ignored while a modal is showing (`dialog_open`). |
| 10 | Accessibility | Window buttons (close / maximise / minimise) had no tooltip and no focus ring. Dialog buttons always reported themselves as enabled. | Tooltip and focus ring added; `wide_button` dims and reports the disabled state. |
| 11 | Robustness | `quick.rs` photo worker called `unwrap()` on a library lookup inside a background thread. | Returns a "was left out" note instead of panicking. |
| 12 | Code quality | The maximised-window check appeared three times. The storage-error-to-words mapping appeared twice. `dropped_files` parsed the extension twice. | `is_maximized`, `storage_reason` and the existing `ext` reused. |
| 13 | Code quality | `toggle_group` detected dark mode with `p.fg == Color32::WHITE`. | Uses `ctx.theme()`. |
| 14 | Tests | 12 UI tests repeated the same 4–5 lines of editor setup. | Replaced by one `open_editor(&mut h, session, layout)` helper. |

## Checked and not a problem

- *Arrow keys on a focused slider also moving the selected item.* egui's
  `egui_wants_keyboard_input()` is true for any focused widget, so the stage
  already ignores the keys. The slider now consumes its arrow keys anyway, and
  `focused_slider_arrow_keys_change_the_value_without_moving_the_item` keeps
  this covered.
- *Delete/Space reaching the editor behind a dialog.* egui's modal already
  takes keyboard focus, so they don't.

## Fixed in the second pass

| # | Area | Finding | Fix |
|---|------|---------|-----|
| 15 | Stage | The first pass called this "text spills out of its selection box". Zoomed in, the box does fit the text: `element_bounds` measures the real layout. The real problem was that the outline hugged the glyphs, so the corner handles covered the first and last letters, and the line used the faint 35% selection fill. | The outline sits `SELECTION_PAD` (6 px) outside the item and uses the solid accent colour. The handles, the rotation knob and the hit-testing all share one `selection_rect`. |
| 16 | Timeline | The playhead was painted over the slide blocks' number and length. | `slide_block` returns where its name goes. `paint_block_name` draws it on a small backing after the playhead, so the line breaks around it. |
| 17 | Slides sidebar | The transition button (`⌄ Slide`) looked like a dropdown of slides. | New `Icon::Transition` (one slide giving way to the next). Tooltip: "Transition into slide 2: Slide. Click to change." |
| 18 | Architecture | Seven playback fields sat loose on `KinettoApp`, and each caller handled the clock and sound card itself. | New `playback::Playback` owns the clock, sound card, cached soundtrack and audio-clock choice. The app calls `start` / `stop` / `tick`, and `start` reports `NoSpeakers` so the app can show its one-time notice. |
| 19 | Performance | `items_list` cloned every element's kind, including the full text, on every frame. | It builds only the `(icon, name, start)` that each row shows. |
| 20 | Platform | The UI font came only from a hard-coded list of file paths. | On Linux/BSD, if none of those exist, it asks fontconfig (`fc-match sans-serif` and `sans-serif:bold`). |

## Still open

- `lib.rs` is still large (~1,700 lines), mostly tests. Moving the `screens`
  test module into `src/tests/` would halve it.
- A non-variable font found through fontconfig without a bold file draws
  "bold" at regular weight. This is the same as before, but now possible on
  more systems.
