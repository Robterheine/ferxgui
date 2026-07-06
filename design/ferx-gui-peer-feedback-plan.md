# Peer feedback triage — root causes and phased plan

**Type:** Bug/UX backlog from external peer review, triaged against the actual `egui`/`eframe` source.
**Scope:** `src/app.rs` (theme, sidebar, header), `src/ui/models_tab.rs` (editor, pills, compare dialog), `src/ui/vpc_tab.rs`, `src/ui/eval_tab.rs`, `src/state.rs`.

Every item below was checked against the code (file:line), not just the bug report text. Two items are genuine functional bugs, not cosmetics — flagged accordingly. Three lenses are used per item: **UI** (visual/interaction design), **PMx** (does this affect a pharmacometrician's trust in / efficient use of the tool), **Rust/egui** (what actually has to change and why).

---

## 1. Sidebar and "Editor/Run/Output/…" pills look like buttons, not tabs

`Tab::ALL` sidebar buttons (`src/app.rs:358-420`) and `ModelPill::ALL` (`src/ui/models_tab.rs:770-782`) are both rendered as free-floating filled/rounded rectangles with no visual connection to the content pane below them — a tab strip should look like part of the container it switches (shared baseline, active tab merges into the content background, inactive tabs recede).

- **UI:** Give the active pill/tab the *same* fill as the content pane behind it (not `ACCENT`) and drop a 2px accent-colored underline instead, à la browser/IDE tabs. Inactive tabs get no fill at all, just dimmed text. This is a pure restyle — no state changes.
- **PMx:** Low functional impact, but a pharmacometrician switching rapidly between Editor/Output/Parameters while iterating on a model benefits from tabs reading as "one strip, one active item" rather than "six buttons, current one highlighted," which is more of a scan-and-click.
- **Rust:** `models_tab.rs:772-779` and `app.rs:374-379` both hand-roll the widget via `allocate_exact_size` + manual paint — restyle is a local color/stroke change in both places, no layout restructure needed.

## 2. Enter in the editor jumps to the Output pill — **functional bug**

Root cause found: `src/ui/models_tab.rs:740-751`, inside `show_model_list` (the model *list*, rendered every frame regardless of which pill is active):

```rust
// Keyboard: Space = toggle star, Enter = switch to Output pill.
if let Some(sel) = state.ui.selected_model {
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    ...
    if enter { state.ui.active_model_pill = ModelPill::Output; }
}
```

`ui.input()` reads **global** key state — it has no idea the Editor pill's `TextEdit` currently has keyboard focus. Every `Enter` keystroke typed into the code editor (`show_editor_pill`, `models_tab.rs:799-901`) is also seen by this list-level shortcut, which immediately flips the pill away from Editor.

- **Rust fix:** Guard the shortcut so it doesn't fire while the editor has focus — simplest correct fix is `if enter && state.ui.active_model_pill != ModelPill::Editor`. (The shortcut is a list-navigation convenience; it makes no sense to fire while looking at a different pill anyway.) A more general fix — checking `ctx.memory(|m| m.focused()).is_none()` before treating Enter as a list shortcut — is more robust if other pills later add their own text inputs, but the pill-check is a one-line, zero-risk change matching the bug exactly.
- **PMx:** This is the single most disruptive item in the list for actual model-editing workflow — every `Enter` while writing a `[pk]`/`[error]` block silently discards editor focus.

## 3. Editor "jumps to next line when just editing" — **functional bug**

Root cause found: `src/ui/models_tab.rs:874-895`. The editor uses a custom `layouter` for syntax highlighting, cached across frames:

```rust
let cache_valid = state.ui.editor_layout_cache.as_ref()
    .is_some_and(|(t, d, _)| t == &state.ui.editor_buffer && *d == dark);
if !cache_valid {
    let job = highlight_ferx(&state.ui.editor_buffer, dark);
    state.ui.editor_layout_cache = Some((state.ui.editor_buffer.clone(), dark, job));
}
let cached_job = ...clone();
let mut layouter = move |ui: &egui::Ui, _text: &str, _wrap: f32| {
    ui.fonts(|f| f.layout_job(cached_job.clone()))
};
let resp = ui.add(egui::TextEdit::multiline(buf).layouter(&mut layouter) ...);
```

`egui::TextEdit` calls the `layouter` closure *during* its own event processing, after it has already applied the keystroke to `buf` — it needs the galley returned by `layouter` to exactly match the *post-edit* text so it can map the new cursor position to a screen row/column. Here the closure ignores the `_text` argument it's given and always returns `cached_job`, which reflects the buffer as of the *previous* frame (the cache is only refreshed once, lazily, before this widget call — not primed with what the edit is about to produce). On any keystroke, especially one that changes line count (typing `Enter`, or typing past a wrapped boundary), the returned galley disagrees with the actual text, and `TextEdit`'s cursor placement — which trusts the galley — lands on the wrong row. That reads exactly as "jumps to next line when just editing."

- **Rust fix:** The caching strategy is sound in principle (recompute only when buffer/theme changed) but must not use a closure that silently substitutes stale data for a *different* string than the one asked about. Two correct options:
  1. Cheapest: make the layouter actually use `_text` — if `_text` matches the cached buffer, return the cached job; otherwise compute fresh (accepting one uncached highlight pass on every keystroke that doesn't match, which is the frame that matters).
  2. Cleaner: drop custom caching entirely and call `highlight_ferx` directly inside the layouter (egui already dedupes identical-text layout requests internally via its own galley cache) — only worth the perf-caution if `highlight_ferx` is measured as slow, which hasn't been checked.
  Recommend (1): smallest diff, preserves the intended caching behavior, fixes the actual mismatch.
- **PMx:** Currently makes the editor close to unusable for anything beyond the shortest scripts — this and #2 should be fixed together since both live in the same widget and a user can't tell which glitch they're hitting.

## 4. Save button hard to read (green on white)

`src/ui/models_tab.rs:825-833` — `egui::Button::new(RichText::new("Save")).fill(theme::GREEN)` sets only the *background* fill; it never sets the label's text color, so text falls back to the theme's default button text color, which is not guaranteed to contrast against `GREEN` (`0x3ec97a`) in light mode.

- **UI fix:** Set explicit text color on the label (`.color(egui::Color32::WHITE)` or a near-black depending on the exact green chosen) rather than relying on theme defaults, and check contrast in *both* themes — this whole class of bug (fill set, text color left to theme default) is worth a quick audit across `models_tab.rs`/`vpc_tab.rs` rather than a single spot-fix, since it's the same mistake as #8 below.

## 5. Warnings hard to read → make orange on black

`src/ui/models_tab.rs:1685-1730`. Warning rows render `theme::ORANGE` text on a *translucent* orange-tinted card (`Color32::from_rgba_unmultiplied(0xe8, 0x95, 0x40, 25)` over the panel fill). In dark mode that's orange-on-near-black (acceptable); in **light mode** the same 10%-alpha orange over a light/white panel produces a pale peach card — orange text on pale peach is exactly the low-contrast case being reported.

- **UI fix:** Make the warning card itself theme-aware instead of a fixed translucent overlay: dark, near-black fill with bright orange text in **both** themes (matching the explicit "orange on black" ask), or at minimum swap to a solid (not alpha-blended) dark card in light mode so text contrast doesn't depend on what's behind it.
- **PMx:** Warnings (boundary hits, high shrinkage, correlation issues) are exactly the signal a pharmacometrician should never have to squint at — worth prioritizing near the top of phase 1.

## 6. VPC tab (and SIR, SimPlot): no visible "which model is selected"

Confirmed by absence: `src/ui/eval_tab.rs:26-27` renders the selected model's stem as a `strong()`, sized label at the top of the tab. `src/ui/vpc_tab.rs:40` and `src/ui/sir_tab.rs:40` both pull the same `stem` from `state.workspace.models[idx].model.stem` — but only ever use it internally (cache keys, file paths); **neither ever puts it in a `ui.label(...)` call.** Grepped both files end-to-end to confirm. Same gap almost certainly applies to `sim_tab.rs`.

- **Rust fix:** Copy the `eval_tab.rs:26-27` pattern (model stem as a `strong()` label, sized ~12-13px) into `vpc_tab.rs`, `sir_tab.rs`, and `sim_tab.rs`, right where each currently does `let stem = ...clone();` — trivial, no state changes needed, data's already there.
- **PMx:** This one matters more than it looks — running a VPC or SIR against the wrong model silently (because you can't see which one is loaded) is a real correctness risk during iterative model development, not just a polish item.

## 7. VPC "Log y-axis" checkbox doesn't visibly change the scale — **likely functional bug**

`src/ui/vpc_tab.rs:745-855`. The log transform is applied *manually* to the y-data before plotting:

```rust
let log_y = opts.log_y && !is_censored;
let ly = |v: f64| -> f64 { if log_y && v > 0.0 { v.log10() } else { v } };
...
if log_y { pl = pl.y_grid_spacer(egui_plot::log_grid_spacer(10)); }
```

`egui_plot::log_grid_spacer` is designed to compute log-scale-styled gridline positions from **raw, untransformed** values (it's meant to be paired with a plot whose *y-axis itself* is log scale, or at minimum a matching axis label formatter that converts the tick position back). Here the plotted data has *already* been `log10`'d by `ly()` before the polygons/lines are built — so `log_grid_spacer` is being asked to lay out log-scale gridlines over data that is already in log space, and there is **no `y_axis_formatter`** anywhere in the file to convert gridline tick values back to `10^value` for display. Net effect: the curves *do* compress correctly (the data transform is right), but the axis gridlines/labels don't read as a log axis (no 1/10/100/1000-style labels) — which is very plausibly what "doesn't change the scale" means from a user's perspective: the shape changed a little, but it doesn't *look* like flipping to log scale the way e.g. R's `ggplot2` `scale_y_log10()` does.
  - Caveat: this is a plausible, code-grounded hypothesis, not confirmed by running the app — worth a 2-minute manual check (toggle the checkbox on a real VPC and screenshot) before fixing, but the mismatch between "pre-transform the data" and "also ask for raw-space log gridlines" is real regardless.
- **Rust fix:** Two internally-consistent options — don't mix them:
  1. Keep the manual `ly()` pre-transform (simplest, already correct for the *data*), drop `log_grid_spacer`, and add a `y_axis_formatter` that renders each gridline's linear-space tick value `v` as `10^v` (e.g. `format!("{:.0}", 10f64.powf(mark.value))`), so ticks read 1/10/100/1000 while internally the grid is evenly spaced in log10 units — which is the correct visual for a log axis.
  2. Or: remove the manual `ly()` transform entirely and rely purely on `log_grid_spacer` over raw data + a genuinely log-scaled plot — bigger change, not clearly better than (1).
  Recommend (1): smallest diff on top of already-correct data handling.
- **PMx:** VPC log-y is a standard, often mandatory view for PK data spanning orders of magnitude (absorption phase vs. terminal phase) — this isn't cosmetic, a VPC that doesn't visibly go log-scale is a real usability gap for the target audience.

## 8. "Continuous" (VPC type toggle) hard to read — light blue on blue, "also in other places"

Root cause is global, not local to VPC: `src/app.rs:65-66`, `apply_dark`:

```rust
v.selection.bg_fill    = ACCENT.linear_multiply(0.4);
v.selection.stroke.color = ACCENT;
```

Any `ui.selectable_label(true, ...)` in dark mode — `vpc_tab.rs:387` ("Continuous"), the Dark/Light theme toggle (`app.rs:628,632`), the subject-per-page combo, the strat-variable combo, etc. — renders `ACCENT`-colored text on an `ACCENT`-at-40%-opacity background: literally blue text on blue. This is exactly the "also in other places" the reviewer flagged; it's systemic, one constant, not a per-widget fix.

- **UI fix:** Change `selection.stroke.color` to something with real contrast against `ACCENT.linear_multiply(0.4)` — `egui::Color32::WHITE` is the standard choice (matches what `widgets.active.fg_stroke.color` already uses at `app.rs:64`), or darken/saturate `selection.bg_fill` further and keep a light text color. Check light mode's `selection` block too (`app.rs:87-88`) for the same pattern before calling this done.
- **Rust:** One-line-ish fix in `theme::apply_dark`/`apply_light`, fixes every `selectable_label` in the app at once — highest leverage single fix in this whole list.

## 9. Individual fits: want 2×2 / 3×3 grid instead of current layout

`src/ui/eval_tab.rs:451,488`: subjects-per-page (`spp`) is user-selectable 1–6 (`ComboBox`, `eval_tab.rs:154-165`), but the column count is hardcoded: `let cols = if spp <= 1 { 1 } else { 2 };` — so `spp=6` renders as a tall 2-wide × 3-tall grid, never a square 3×3, and there's no 9-subject option at all.

- **UI/Rust fix:** Replace the flat 1–6 "per page" count with an explicit small set of grid shapes (e.g. 1×1, 2×2, 3×2, 3×3 = 9), or keep a count but derive `cols = (spp as f32).sqrt().ceil() as usize` so larger counts trend toward square grids automatically. Extending to 9 needs `state.ui.eval_subjects_per_page` (`state.rs:407`) to allow values up to 9 — currently clamped `1..=6` at `eval_tab.rs:451` and the combo only offers `1..=6` (`eval_tab.rs:158`).
- **PMx:** More subjects visible per screen materially speeds up eyeballing individual fits across a full dataset — cheap, well-scoped win.

## 10. Menu bar (File / Edit / View / About) — expectation-setting native convention

No menu bar exists today — confirmed no `egui::menu::bar`/`menu_button` at the top level in `app.rs`; the only entry points are the sidebar icons and a small "About" button in the header (`app.rs:318-320`).

- **UI/PMx:** Reasonable ask — desktop users expect it, and it's a natural home for things like Settings (`Cmd+,`, item 11) and About, decluttering the header.
- **Rust — important caveat:** `egui`/`eframe` menu bars (`egui::menu::bar` + `ui.menu_button`) render an **in-window** bar, not a true native macOS `NSMenu` in the system menu bar. Getting an actual native macOS menu bar (the thing "File Edit View" usually means to a Mac user) requires platform-specific integration (e.g. the `muda` crate wired through `winit`/`eframe`'s window handle), which is a materially bigger lift than adding an in-app bar. **Recommend clarifying with the user which they want** before starting — an in-app bar is a half-day change; a true native macOS menu bar is a separate, larger piece of work with its own platform-integration risk.

## 11. Settings should be a modal, not a tab, bound to `Cmd+,`

Confirmed: `Tab::Settings` is a full entry in `Tab::ALL` (`state.rs:38`), rendered in the sidebar like any other tab, body via `render_settings(ui, state)` (`app.rs:457`). No `Cmd+,`/`Ctrl+,` handling exists anywhere — the only global shortcuts today are `Ctrl+1..9` (tab switch, `app.rs:196-215`) and `Ctrl+S` (save, `models_tab.rs:839`).

- **Rust fix:** Convert `render_settings` into an `egui::Window` shown via a `state.ui.settings_open: bool` flag (same pattern already used for `run_popup_open`/`sir_popup_open`/`about_open`, all in `state.ui` — this is a well-established pattern in this codebase, not a new one), remove `Tab::Settings` from the sidebar, and add a modifier check for `,` alongside the existing `Ctrl+1..9` block in `app.rs:196-215` (`Cmd` on macOS is `Modifiers::MAC_CMD`/`i.modifiers.command` in `egui`, which already maps correctly cross-platform without extra work).
- **UI:** Low risk, mechanical change — this is a good candidate to bundle with item 10 since both touch the header/global-shortcut area.

## 12. Model comparison — partially exists, needs to be extended and surfaced

Not a gap from scratch: `src/ui/models_tab.rs:3152-3279` already implements a compare dialog — triggered via right-click → "Compare with…" on a model row (`models_tab.rs:526-530`), showing ΔOFV/ΔAIC/LRT and a full THETA/OMEGA/SIGMA parameter table with RSE% and Δ%/Δ-abs per parameter (`compare_param_rows`, `models_tab.rs:3282+`). This is solid, non-trivial existing work.

**Gaps versus the ask:**
- **Discoverability:** only reachable via right-click context menu on a row — a peer reviewer testing the app plausibly never found it. Consider surfacing it as a normal action too (e.g. multi-select two rows + a "Compare" button/toolbar action), not only a hidden submenu.
- **No GOF plot comparison:** the dialog is parameter-table only; no overlaid/side-by-side GOF plots (DV vs. PRED/IPRED, CWRES, etc.) or VPC comparison. This is the larger missing piece.
- **PMx:** Parameter-table comparison (with LRT) is actually the harder, more valuable part and it's done — GOF-plot comparison is normally a secondary confirmation step, so the existing feature covers the primary use case already; it's an extension, not a rebuild.
- **Rust scope estimate:** Reuse `eval_tab.rs`'s existing GOF plotting code (`show` dispatches to GOF section) rather than writing new plot code — the compare window would need to fetch `eval_data`-equivalent series for both `fit_a`/`fit_b` and overlay or tile them. This is a meaningfully bigger piece of work than anything else in this list; scope it as its own follow-up rather than folding into the same pass as items 1-11.

---

## Phased plan

**Phase 0 — correctness bugs (do first, independent of each other, low risk):**
- #2 Enter-key-steals-focus guard (one-line)
- #3 Editor layouter staleness (small, contained to `show_editor_pill`)
- #7 VPC log-y axis formatter (contained to `vpc_tab.rs` render fn)

**Phase 1 — contrast/readability (small diffs, high visible impact):**
- #8 `selection` stroke/bg contrast fix in `theme::apply_dark`/`apply_light` (fixes #8 *and* incidentally most of the "also in other places" instances)
- #4 Save button text color
- #5 Warning card theme-aware contrast
- #6 Selected-model label in VPC/SIR/SimPlot tabs (copy `eval_tab.rs` pattern, 3 call sites)

**Phase 2 — layout/style (moderate, self-contained):**
- #1 Tab-strip restyle (sidebar + model pills)
- #9 Individual-fits grid shape (extend `eval_subjects_per_page` range + column calc)

**Phase 3 — structural additions (bigger, needs a decision first):**
- #11 Settings → modal + `Cmd+,` (mechanical, low risk, do anytime)
- #10 Menu bar — **needs a decision**: in-app `egui` menu bar (half-day) vs. true native macOS menu bar via `muda` (separate, larger effort). Recommend starting with the in-app bar and revisiting native integration only if it's a hard requirement.
- #12 Model comparison extension (surface the existing feature + add GOF-plot comparison) — largest single item, scope as its own follow-up design pass once phases 0-2 are done, reusing `eval_tab.rs` GOF plotting code rather than duplicating it.

Nothing above has been implemented yet — this is the triage/plan only, per the request.
