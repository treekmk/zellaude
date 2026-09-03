## Plan: Layout Generator

Replace enumerated custom states with generator files: one KDL file per generator under `~/.config/zellij/plugins/zellaude/generators/`, invoked from the existing `Ctrl+t` `Shift+n` prompt with a CLI-style line (`impl 4 --crit-per-impl 2`). A small declarative language (`tab`/`pane`/`each`, `if`/`unless`, integer ranges) expands the file into one or more tabs, named `<source tab>-1`, `-2`, … unless a name template using `{tab}` and the loop variables says otherwise; the plugin lays each tab out from the live tab size under minimum-pane floors and refuses when it does not fit. Generator files and the custom-state portion of `zellaude.json` are re-read on every prompt open, so edits need no new session. No new compiled dependency: the `kdl` crate already reaches the plugin through zellij-utils and needs only a manifest line.

**Context**
Custom states are exact-id lookups of fixed shapes (`src/custom_layouts.rs`), so a madev run of *n* implementers with *m* critics laid out per role needs ~108 enumerated states, none adapting to the terminal it opens on (seed `feature/layout-generator`; dotfiles seed `feature/seed-madev-layouts` is gated on this). The plugin already holds the tab's real size (`TabInfo.display_area_rows/columns`), which an external generator cannot see. The language is generic: madev naming lives in the user's file, never in the plugin.
Branch `feature/layout-generator`; the merge task merges `develop` in.
Mode: ask

**Approach**
- Phase 1 (T1–T3, impl1): the `kdl` manifest line, then new module `src/layout_generators.rs` — parse a generator file (declaration + body), parse a prompt line against the declared arguments, expand the body into per-tab command lists, plan each tab's grid from geometry and floors → `Vec<CustomLayout>`.
- Phase 2 (T4, impl2): `src/custom_layouts.rs` — multi-tab KDL emitter; `CustomLayout::to_kdl` delegates to it; `TabChrome::bar_rows`.
- Phase 3 (T5–T6, impl2): `src/main.rs` + `src/state.rs` — reload on prompt open through one `run_command` returning `zellaude.json` plus the generator files; a result arm that touches only the custom-state portion; submit deferred while a reload is in flight, then resolved exact id first, generator second, with geometry from `TabInfo`; `mod layout_generators` wired.
- Phase 4 (T7 impl1, T8 impl2): README; E2E harness under `/tmp/layout-generator/e2e/`.
- Finalize (T9–T12, impl3).
- Cross-phase: T6 waits on T3 (generator API) and T4 (emitter); T7 on T3; T8 on T6.

**Relevant files**
- `src/layout_generators.rs` — NEW: file schema and parse, prompt-line argument parsing, expansion, grid planning, floors. Depends only on `custom_layouts::{CustomLayout, CommandGrid, MAX_PANES}`; no host calls, no `crate::state`, so the test harness can include it by `#[path]`.
- `tests/layout_generators.rs` — NEW: `#[path]`-includes `custom_layouts` and `layout_generators` (pattern of `tests/session_templates.rs:1-8`); asserts geometry by round-tripping the emitted KDL through `zellij_utils::input::layout::Layout::from_kdl` + `position_panes_in_space` (pattern of `tests/custom_layouts.rs:536-584`).
- `src/custom_layouts.rs` — `tabs_to_kdl`; `to_kdl` (`:213-285`) delegates; `TabChrome::bar_rows`. `CustomLayout`'s fields are unchanged (test literals lack `..Default::default()`).
- `tests/custom_layouts.rs` — multi-tab emission tests; the ten existing `CustomLayout::to_kdl` call sites (eight here, two in `src/main.rs`) stay.
- `src/main.rs` — `RELOAD_CUSTOM_STATES_SCRIPT` beside `SAVE_CONFIG_SCRIPT` (`:27`); `start_custom_layout_prompt` (`:637`) issues the reload; `open_custom_layout` (`:719`) defers or resolves; new `RunCommandResult` arm `reload_custom_states` beside `load_config` (`:311`); `mod layout_generators`.
- `src/state.rs` — new `State` fields (below).
- `Cargo.toml` — `kdl = "4.7"` (already pinned in `Cargo.lock` by zellij-utils); owned by impl1 in T1, since the module and its test harness cannot compile without it.
- `README.md` — "Custom states" (`:55-111`): "Reload the plugin" (`:79`) replaced by hot reload; the Zellij-fit sentence (`:83`) replaced by the floors; new "Generators" subsection; Features bullet (`:16`).
- Not touched: `src/render.rs` (see **Render untouched**), `src/session_templates.rs`, `install.sh`, `scripts/`.

**Naming & signatures**

User-facing vocabulary (locked; the README documents exactly this):

```kdl
// ~/.config/zellij/plugins/zellaude/generators/madev.kdl — the canonical example: declaration nodes, then body nodes
command "impl"                                    // first word of the prompt line; unique across files
arg "n"                                           // positional integer, required, in declaration order
flag "crit-per-impl" value="m" default=1          // --crit-per-impl <int>; default when absent
flag "single-tab"                                 // presence variable single_tab ('-' becomes '_')
flag "only-crit" optional-value="from" default=1  // --only-crit [<int>]: presence only_crit, integer from
min_pane_width 54                                 // optional per-file floors, content cells
min_pane_height 12

tab "{tab}-impl" unless="single_tab only_crit" {  // name = template over {tab} (source tab's name) and loop variables; if="a b": all present; unless="a b": none present
    each for="i" in="1..=n" {                     // a..b / a..=b, Rust semantics; endpoint = term [('+'|'-') term], term = int | name; one operator, no chaining
        pane "claude -n impl{i}"                  // {name} substituted for declared names only; {tab} arrives shell-quoted here; other braces verbatim
    }
}
each for="k" in="from..from+m" {
    tab "{tab}-crit{k}" unless="single_tab" {
        each for="i" in="1..=n" {
            pane "claude -n impl{i}-crit{k}"
        }
    }
}
tab if="single_tab" {                             // no name: "<source tab>-<ordinal>", 1-based, in emission order
    each for="i" in="1..=n" {                     // impl-major: each implementer beside its critics
        pane "claude -n impl{i}" unless="only_crit"
        each for="k" in="from..from+m" {
            pane "claude -n impl{i}-crit{k}"
        }
    }
}
```
KDL needs a node terminator before a closing brace on the same line, so README examples and test fixtures are written multi-line as above (a one-liner needs `;`, as in `tab { pane "a"; }`).
- Structure: declarations precede body nodes. `each` nests anywhere; `tab` only at top level or under an `each` chain rooted at top level, never inside a `tab`; `pane` only inside a `tab`, possibly under `each`. `if`/`unless` are accepted on every body node alike — `tab`, `pane`, and `each` (a false condition on `each` skips its whole expansion). Unknown node, property, or variable name is a parse error. Variables: positionals, flag values, flag presences, `each` loop variables, and the built-in `tab` (the source tab's name, the only string); all others are integers or presences.
- Flags: every flag declares a presence variable named after it (`-` → `_`); `value` / `optional-value` add the named integer as well. A `value` flag without `default` is required on the prompt line; an `optional-value` flag without `default` is a file parse error, since the bare form would leave its integer unbound. Presences are legal only in `if`/`unless`; integers are legal everywhere except `if`/`unless`; `tab` is legal only in name templates and commands (never in `if`/`unless`, never as a range endpoint); every misuse is a parse error.
- Prompt line: tokens split on whitespace; first token is the command; positionals fill in order and may sit among flags; flags in any order. Unknown flag, missing required flag, missing or non-integer value, or leftover token refuses.
- Empty ranges expand to nothing; a tab with no panes, or an invocation with no tabs, refuses.
- Tab names: a `tab` without a name argument is named `<source tab name>-<ordinal>`, the ordinal counting emitted tabs from 1 in emission order. An explicit name is a template over `{tab}` and the loop variables in scope. `{tab}` is inserted raw in a name and single-quoted (embedded quotes escaped) in a `pane` command, so a tab name can never alter how `sh -lc` parses the command.
- Grid per tab with *n* panes on a tab of `W`×`H` cells: `c_max = floor(W / (min_pane_width + PANE_FRAME_COLUMNS))`, `r = ceil(n / c_max)`, *n* split into *r* rows as evenly as possible with the larger rows last; refuse when `c_max = 0` or `floor(H / r) - PANE_FRAME_ROWS < min_pane_height`. Emitted as `CommandGrid::Rows`. `MAX_PANES` and the byte caps apply per tab.
- Every generator refusal is prefixed with the file's basename and contains `does not fit` when a floor is the cause. The generator validates each resolved tab name itself (non-empty, at most `MAX_ID_CHARACTERS`, no control characters) before it becomes a `CustomLayout` id, so a bad `{tab}` refuses in the generator's voice, never in `CustomLayout::validate`'s. A single `each` range is bounded before iteration: more than `MAX_PANES` steps refuses (`range "1..=n" runs for N steps; the maximum is 64`), since a tab cannot hold more and more tabs than that from one prompt is not a use case; a prompt-line typo like `impl 1000000000` therefore never allocates (nested `each` levels inside a trusted file multiply past it and are caught by the per-tab cap after expansion). The one unprefixed refusal is the no-match case: when no `command` equals the first token, `invoke` returns `Unknown custom state or generator "…"` (no file matched), and `resolve_custom_state` passes every `invoke` error through verbatim, retiring today's `Unknown custom state` string.

```rust
// src/layout_generators.rs — one generator = one .kdl file, re-parsed on every prompt open
pub const DEFAULT_MIN_PANE_WIDTH: usize = 54;    // content columns
pub const DEFAULT_MIN_PANE_HEIGHT: usize = 12;   // content rows
pub const PANE_FRAME_COLUMNS: usize = 2;         // Zellij frame cost per pane
pub const PANE_FRAME_ROWS: usize = 2;

#[derive(Deserialize)]
pub struct GeneratorFile { pub path: String, pub content: String }   // one entry of the reload envelope
#[derive(Deserialize)]
pub struct CustomStateSources { pub settings_json: String, pub generator_files: Vec<GeneratorFile> }  // the envelope the scan script prints; files in LC_ALL=C name order

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct FloorOverrides { pub min_pane_width: Option<usize>, pub min_pane_height: Option<usize> }  // one file's or zellaude.json's partial floors
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PaneFloors { pub min_pane_width: usize, pub min_pane_height: usize }                       // resolved: file → zellaude.json → constants
impl PaneFloors { pub fn resolve(file: FloorOverrides, global: FloorOverrides) -> Self { ... } }
pub fn parse_floor_overrides(settings_json: &str) -> Result<FloorOverrides, String> { ... }          // top-level keys of zellaude.json; absent → None; non-integer → Err

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TabGeometry { pub columns: usize, pub rows: usize }   // cells the grid may use: display area minus the bar rows the new tab carries
#[derive(Clone, PartialEq, Eq)]
pub struct SourceTab { pub name: String, pub geometry: TabGeometry }   // the tab the prompt was opened from: bound as {tab}, and sizes every generated tab

pub struct LayoutGenerator { pub command: String, pub source: String, ... }   // source = file basename, for messages; args, flags, floors, body private
pub fn parse_generator_files(files: &[GeneratorFile]) -> Result<Vec<LayoutGenerator>, String> { ... }   // order kept; duplicate command → Err naming both files
pub fn invoke(generators: &[LayoutGenerator], input: &str, source: &SourceTab, global_floors: FloorOverrides) -> Result<Vec<CustomLayout>, String> { ... }   // impl1 produces; impl2 calls from the submit path; one CustomLayout per emitted tab, id = the resolved tab name
pub fn plan_rows(pane_count: usize, geometry: TabGeometry, floors: PaneFloors) -> Result<Vec<usize>, String> { ... }   // panes per row, larger rows last
```

```rust
// src/custom_layouts.rs
pub fn tabs_to_kdl(tabs: &[CustomLayout], plugin_location: &str, plugin_configuration: &BTreeMap<String, String>, cwd: Option<&str>, chrome: &TabChrome) -> Result<String, String> { ... }   // one `tab name=<id>` per entry, focus=true on the first only; to_kdl delegates with slice::from_ref
impl TabChrome { pub fn bar_rows(&self, plugin_location: &str) -> usize { ... } }   // top + bottom, plus one when the plugin's own bar is absent from top (to_kdl inserts it)
pub struct Prompt { ..., pub pending_submit: Option<String> }   // new field, set in Prompt::new; input submitted while a reload was in flight, resolved when the result lands
```

```rust
// src/state.rs — State
pub layout_generators: Vec<layout_generators::LayoutGenerator>,
pub layout_generator_config_error: Option<String>,
pub pane_floor_overrides: layout_generators::FloorOverrides,   // from zellaude.json
pub custom_state_reload_in_flight: bool,
// src/main.rs
const RELOAD_CUSTOM_STATES_SCRIPT: &str = ...;   // sh + jq: prints the CustomStateSources envelope; zellaude.json missing → "{}"; *.kdl only, LC_ALL=C order
fn reload_custom_states(&mut self) { ... }                                   // run_command with context type "reload_custom_states"; sets the in-flight flag
fn apply_custom_state_sources(&mut self, sources: &CustomStateSources) { ... }   // custom_layouts (plugin-block precedence as today), floors, generators, both error slots; refreshes an open prompt's empty-input hint. The result arm resolves pending_submit itself, on the success and the failure path alike, so a submit deferred by a failed read is not swallowed
fn resolve_custom_state(&self, input: &str, source: &SourceTab) -> Result<Vec<CustomLayout>, String> { ... }   // exact custom_states id → clone; else layout_generators::invoke
fn prompt_source_tab(&self, tab_position: usize, chrome: &TabChrome) -> Option<SourceTab> { ... }   // TabInfo.name and display_area_* of that position, rows minus chrome.bar_rows
```
- `open_custom_layout` finds the source tab with `manifest.panes.iter()` (today `.values()`, `src/main.rs:735`, which drops the position key `TabInfo` is looked up by).
- Name choices: module `layout_generators` mirrors `session_templates`; `PaneFloors`/`min_pane_*` say "pane content floor" so they cannot be read as the grid `width`/`height` that `CustomLayout` and `TemplateTab` already use; `CustomStateSources` names what the envelope is, not how it is fetched.

**Verification**
- `cargo test --target x86_64-unknown-linux-gnu --features zellij-utils/vendored_curl` exit 0. Capture to a file and sum the `test result:` lines (one per binary); never `tail`.
- `cargo build --release --target wasm32-wasip1` exit 0; report the wasm size delta against `develop` (expect well under 100 KB).
- Unit coverage, `tests/layout_generators.rs`: parse errors (unknown node/property/variable, duplicate variable, `tab` inside `tab`, `pane` outside `tab`, a body node before a declaration, `optional-value` without `default`, a presence outside `if`/`unless`, an integer inside `if`/`unless`, `tab` inside `if`/`unless` or as a range endpoint); prompt-line parsing (flags in any order, every flag's presence bound, optional value present/absent, defaults, a `value` flag without `default` required and its `missing required flag` refusal, unknown flag, missing value, non-integer, leftover token); ranges (`..`, `..=`, `+`/`-` with an integer and with a variable, overflow and negative bound refuse, a range over `MAX_PANES` steps refuses before iterating, empty); the no-match message for an unknown first token; `if`/`unless` all/none, and a condition on each of `tab`, `pane`, and `each` (a false `each` condition emits nothing); nested `each` order (i-major); `plan_rows` table (W=284: c_max=5, n=7 → [3,4], n=12 → [4,4,4]; width refusal at c_max=0; height refusal); floors chain file → global → constants; duplicate `command` across files names both; `{i}` substituted while `${HOME}` and `{a,b}` stay; unnamed tabs come out as `<source>-1`, `<source>-2` in emission order while `tab "{tab}-crit{k}"` resolves the template; `{tab}` inside a command is single-quoted with embedded quotes escaped, checked through `extract_run_instructions` with the source names `it's` and `x'; id; '` (the quote is the only character that can leave a single-quoted string, so these are the names that probe the escaping); a source name over `MAX_ID_CHARACTERS` or holding a control character refuses with the file prefix; the madev file from the README expands `impl 4 --crit-per-impl 2` to tabs `<source>-impl`, `<source>-crit1`, `<source>-crit2`, `impl 4 --single-tab` to one 8-pane tab in impl-major order (impl1, impl1-crit1, impl2, …), and `impl 4 --crit-per-impl 2 --only-crit 2` to tabs `<source>-crit2`, `<source>-crit3` only; every generated tab round-trips through `Layout::from_kdl` with the expected positioned geometry.
- Unit coverage, `tests/custom_layouts.rs`: `tabs_to_kdl` with two tabs parses with `parsed.tabs.len() == 2` and exactly one focused tab; a one-tab call is byte-identical to `to_kdl`; `bar_rows` with and without the plugin's own bar.
- Unit coverage, `src/main.rs` inline: `RELOAD_CUSTOM_STATES_SCRIPT` run through real `sh` against a temp `HOME` (pattern of the `save_config` tests, `src/main.rs:2004-2120`): missing file → `settings_json == "{}"`, files in byte order, non-`.kdl` ignored. `apply_custom_state_sources` on a `State`: a document without `custom_states` clears previously loaded states, a document that fails to parse keeps them and fills the error slot, a bad generator file keeps the last good generators and fills `layout_generator_config_error`, and with `custom_layouts_from_plugin_configuration` set a document without `custom_states` leaves the plugin-block states intact.
- **E2E** — a real Zellij session drives the built plugin through the prompt.
  - Setup: build the wasm; stage `HOME=/tmp/layout-generator/e2e/home` (never the real one) with `.config/zellij/plugins/zellaude.json` = `{"min_pane_width": 100, "min_pane_height": 5}`, `generators/grid.kdl` (`command "grid"`, `arg "n"`, `flag "tabs" value="t" default=1`, `each for="k" in="1..=t" { tab { each for="i" in="1..=n" { pane "printf g{k}-p{i}; sleep 600" } } }` written multi-line in the file, tabs deliberately unnamed), `.cache/zellij/permissions.kdl` granting the built wasm's absolute path the seven `REQUIRED_PERMISSIONS`, and `e2e.kdl` = the repo's `layout.kdl` pointing at that wasm plus a `tab name="e2e"` node so the source tab has a known name.
  - Drive: a python `pty.fork` client (200 columns × 50 rows via `TIOCSWINSZ`) runs `zellij --session zellaude-e2e-lg --layout e2e.kdl` under the staged `HOME`, waits for the bar, and before every case returns focus to the `e2e` tab (`zellij -s zellaude-e2e-lg action go-to-tab-name e2e`, confirmed by the `FOCUSED` column of `list-panes` before typing, since case 1 leaves focus on the first generated tab), then sends `Ctrl+t`, `N`, types the line, `Enter`, and captures the screen. Assertions read `zellij -s zellaude-e2e-lg action list-panes --all --tab --geometry`.
  - Cases and pass conditions: (1) `grid 4 --tabs 2` → tabs `e2e-1` and `e2e-2` exist (the default names), each with exactly four terminal panes titled `printf g1-p1; sleep 600` …, every pane `COLS` = 200 and `ROWS` ≥ 11 (width floor 100 forces one column, four rows). (2) `grid 9` → tab count unchanged after 3 s and the captured screen contains `does not fit`. (3) write `generators/hot.kdl` (`command "hot"`, `arg "n"`, `tab "{tab}-hot"` holding *n* panes) after the session is up, then `hot 2` → tab `e2e-hot` with two panes, proving the reload without a restart and the `{tab}` template. Exit 0 only when all three hold.
  - **Preflight**: `zellij --version` starts with `0.44`; `python3 -c 'import pty'`; `jq --version`; no session named `zellaude-e2e-lg` in `zellij list-sessions`; the wasm exists. No GPU, RAM, port, or quota concerns. Cleanup: `zellij delete-session zellaude-e2e-lg --force`.
  - **Artifacts:** `/tmp/layout-generator/e2e/` — the driver, the staged `HOME`, `list-panes` dumps per case, the pty capture, `zellij.log` excerpt from `/tmp/zellij-<uid>/zellij-log/`. Never in the repo, never staged.

**Decisions**
- **Generator language (user-decided):** per-file KDL generators with a declarative body — not JSON index templates (every variant an entry; placeholder vocabulary grows), not a plugin-run user script (async submit across a host round-trip with no timeout; a second contract). The language is data the plugin interprets: no eval anywhere, so a ported file can express nothing the plugin does not already do.
- **Integer and presence variables, plus one quoted string:** a substituted integer is a digit string, so it cannot change how `sh -lc` parses a command. `{tab}` is the only string and is single-quoted inside commands (raw in tab names, which are not shell), so the guarantee holds even for a hostile tab name. The only code in a file is its pane commands — the same trust `custom_states` already carries (README "trusted local shell code").
- **Source tab name `{tab}` and default tab names (user-decided):** the tab the prompt was opened from is bound as `{tab}`, usable in tab-name templates and commands, like the `cwd` the generated tabs already inherit. A `tab` node without a name is named `<source>-<ordinal>` so the common case needs no naming at all. Fixed `custom_states` keep their id as the tab name.
- **Declared arguments, not a regex:** the user wants flags in any order, which a regex cannot give without an argument parser anyway. Parsing whitespace-split tokens against `arg`/`flag` declarations is ~100 lines with no dependency; `regex-lite` (94 KB) and `clap` (large, multi-line errors) were both declined.
- **KDL, not YAML or JSON:** the `kdl` crate is already compiled into the plugin (zellij-utils), Zellij users already write it, and the plugin already emits it. Rust's YAML crates are unsettled (`serde_yaml` deprecated, `serde_yml` unmaintained); JSON makes the declarations noisy. zellij-utils does not re-export `kdl` (checked), hence the manifest line.
- **Ranges `a..b` / `a..=b` with one `±` term:** Rust semantics, chosen by the user; step was dropped. An endpoint is `term [('+'|'-') term]`, each term an integer literal or a variable, so the seed's `--only-crit` shape (`from..from+m`) parses. Endpoints are computed at expansion with checked arithmetic (a negative or overflowing bound refuses); no user code is ever evaluated.
- **Hot reload is a separate read:** re-firing the `load_config` result arm is unsafe: at `src/main.rs:347` it calls `on_command_permissions_granted`, which calls `set_selectable(false)` (`:1491`) under an open prompt; it overwrites `Settings` racing an async `save_config` (`:1853`); it double-reports template errors (`:334-341`). The new arm applies only the custom-state portion, and applies what parsed: `Ok(Some)` replaces the states and clears the error slot; `Ok(None)` (key removed) clears both, as a fresh instance would hold; `Err` keeps the last good set and fills the error slot, the stance the repo already takes for session templates. All three arms, `Ok(None)` included, are guarded by `!custom_layouts_from_plugin_configuration`: when the states came from the plugin block the file's `custom_states` portion is ignored entirely (today's `Ok(None)` arm at `src/main.rs:321` is an unguarded no-op only because the map is empty at load time). The same rule governs `parse_generator_files` and `parse_floor_overrides` into `layout_generator_config_error`; neither has a plugin-block form.
- **Reload scope (user-decided):** generator files plus `custom_states` and the floor keys of `zellaude.json`, in one round trip; session templates stay once-per-instance (a new tab is a fresh instance that recompiles them, and a prompt open must not write layout files).
- **Reload trigger:** on prompt open, on demand. No polling: the wasm plugin has no file watch and a timer `stat` is waste. Submit during a reload is deferred (`pending_submit`) and resolved when the result lands; a prompt cancelled meanwhile drops it. One boolean flag: the window is one `cat`, so overlapping reloads apply in arrival order and are accepted.
- **Directory and envelope:** `~/.config/zellij/plugins/zellaude/generators/*.kdl`, files only — no inline generators in `zellaude.json` or the plugin block. The scan script uses `jq` (already required by `SAVE_CONFIG_SCRIPT`) to print one JSON envelope carrying the raw `zellaude.json` text and each file's content, so the existing parsers keep receiving the raw document.
- **Floors (user-decided):** per-file → `zellaude.json` top-level `min_pane_width`/`min_pane_height` → constants 54/12. `zellaude.json` is never installed (`install.sh` does not touch it; the plugin reads `{}` when absent, `src/main.rs:1847`), so the constants are the always-present layer. The keys live in the custom-state portion, not in `Settings`, so a settings save never rewrites them (`serialized_settings`, `:1877`).
- **Geometry source:** `TabInfo.display_area_columns/rows` of the prompt's tab, minus the bar rows the new tab will carry. `viewport_*` was rejected: its accounting of plugin bars is undocumented. Frame cost is a constant 2 columns / 2 rows per pane (seed-verified: a 284-column tab yields 140 content columns at two columns; live check 2026-09-02 showed 142 outer). Measuring frames from a live pane was declined: with frames off the estimate is merely conservative.
- **Lookup precedence:** exact `custom_states` id first, then the generator whose `command` equals the first token, so a literal id such as `2fa` keeps working. Two files declaring the same command is a config error naming both files, not first-wins: a silent shadow is the porting mistake the user wants surfaced.
- **Multi-tab emission:** `focus=true` on the first tab only — Zellij rejects a document with two (`kdl_layout_parser.rs:2530`, "Only one tab can be focused"). Every tab gets the prompt's `cwd`, as today. Generated tab names need not be unique (nothing in the plugin keys on tab names). `new_tabs_with_layout`'s return stays unused, as today (`src/main.rs:770-777`).
- **Render untouched:** no "reloading…" hint. The bar is one row and shows an error in at most half its width (`src/render.rs:441`), so refusal messages stay short. `tests/render_theme.rs` stubs `Prompt` and `State` with only the fields render reads, so new fields go into `Prompt::new` and `State` but render never reads them.
- **Probe verdicts:** multi-tab KDL through `new_tabs_with_layout` creates every named tab (verified in a live session per the seed); the `kdl` 4.7.1 API (`KdlDocument::from_str`, `node.entries()`, `node.children()`) covers the vocabulary (read in the registry source); no runtime probe was needed.
