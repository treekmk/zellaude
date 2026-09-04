## Plan: Non-blocking attach scan

The attach scan asks Zellij synchronously what runs in every pane, two asks per
pane. A blocked ask is given up on after **5 s** — measured, `GetPanePid timed
out after 5s` — so a 20-pane session can park the plugin thread far longer than
the ~20 s actually observed in the incident. (The 1 s figure quoted elsewhere is
Zellij's *Action* give-up, a different timer; see the E2E notes.) Delete all
three blocking host-call sites; let the attach script derive the pane→process
map from `/proc/*/environ`, which Zellij already fills with
`ZELLIJ_SESSION_NAME` and `ZELLIJ_PANE_ID`. Measured here: 22 ms for 528
processes.

**Context**
Diagnosed 2026-09-01 from a freeze in session
`rational-diophantine-n-tuple` (4 tabs, 20 terminal panes, ~13 agents): bar
panes flickering to Zellij's `Loading …` placeholder, `zellij a` returning `Bye
from Zellij!` at once, `zellmv park` dying on an empty layout dump. One cause:
`attach::run` parks the plugin thread on one synchronous host call per pane.
`get_pane_pid` is a stdout-write then stdin-read round trip
(`zellij-tile-0.44.3/src/shim.rs:1876`); the instance waits idle, and Zellaude
registers no worker. Every re-attach re-arms it. Wanted: discovery works as
today, no blocking host calls. Branch `feature/nonblocking-attach-scan`; the
merge task merges in `develop`. Mode: ask

**Approach**
- Phase 1 (T1–T3, script): fix the `comm` basename match in `find_agent_pid`
  first — until it lands the validator does nothing on macOS. Then one `/proc`
  pass maps pane → processes, replacing the pane-record drive loop; pick agent
  processes from each pane's set. Then replace the cached-state cross-check with
  the common pid-liveness validator. Argv `$2` loses its last reader and goes.
- Phase 2 (T4, plugin): delete the three blocking call sites and the four
  symbols they orphan; fix comments and README lines calling pane introspection
  live.
- Disjoint files; no ordering *between* the phases, though Phase 1 is ordered
  internally as above. Coherent only once T3 and T4 both land — until then the
  plugin passes a record argument the script ignores. Nothing exercises that
  pair before verification, so neither agent should call it a break.
- Finalize (T5–T8): merge `develop`, compact comments, verify, archive.

**Relevant files**
- `scripts/zellaude-attach.sh` — discovery script, embedded in the plugin by
  `include_str!`. Gains the `/proc` walk; loses `record_client_for_pane`,
  `foreground_pid`, the depth-64 parent climb.
- `tests/attach_detection.sh` — the only test driving it, via a fake
  `$PROC_ROOT`. All ten cases get rewired: today's fixtures write `environ` only
  for the agent pids (101/201), so every pane's process set must be modelled.
- `scripts/zellaude-hook.sh` — `find_agent_pid` (`:283-285`) matches `comm`
  unnormalized, so on macOS it never matches and `agent_pid` is written null
  (`:933`) for every entry. Same basename rule as the validator.
- `tests/hook_mode_detection.sh` — coverage for that fix. Both files are hot:
  +196 and +450 lines across this session's span, so expect merge friction.
- `src/attach.rs` — `run()` loses the host-call loop and its
  `supports_introspection` parameter; `supports_pane_introspection` and
  `client_for_command` go with their unit tests.
- `src/main.rs` — the `attach_scan` arm loses the post-scan `get_pane_pid`
  re-check and its two context keys; `introspection_supported()` goes.
  `get_zellij_version` stays — `split_three` calls it at `:142`.
- `src/state.rs` — drops `pane_introspection_supported`.
- `README.md:588-589`,
  `docs/superpowers/specs/2026-07-31-better-codex-support-design.md` — the only
  prose this feature makes stale: README describes attach recovery as using
  "Zellij's pane PID and procfs", and the spec's deliverable is
  `client_for_command`, which this deletes.

**Explicitly out of scope: poll-era introspection prose.** Four comments speak
of pane introspection — `src/state.rs:63`, `src/main.rs:2002`,
`tests/event_handler.rs:471`, and `src/placeholder.rs:5-10`. All four describe
the introspection *poll*, removed earlier in commit `2220586`, not the host
calls this change deletes, so none becomes stale here. `placeholder.rs` is
already correct as written ("Nothing creates these since the introspection poll
was removed") and must not be rewritten. Leave all four alone:
`tests/event_handler.rs` belongs to no agent's file set, so tidying it would be
a stray edit in an unowned file, and the compact-comments row covers touched
files only and will not reach it either.

**Naming & signatures**

Script argv contract, replacing the three-argument form at
`scripts/zellaude-attach.sh:3-6`:

```sh
# $1 — Zellij session name
# $2 — attach scan start time in milliseconds   (was $3)
```

```sh
# DISCOVERY — Linux only, behind the uname gate at :141. One /proc pass.
# Prints "pane_id<TAB>pid<TAB>client" for every claude/codex process whose
# ZELLIJ_SESSION_NAME is $SESSION_NAME. Empty on macOS, which simply means no
# enrichment there — it never gates the validator below.
list_pane_processes() { ...; }
PANE_PROCESSES=...

# VALIDATION — common to both platforms, no uname branch. True ONLY on positive
# evidence that the cached entry is dead: ps exited 1 with empty output, or the
# normalized comm does not match "<client>*". Every other outcome — exit 127,
# any other status, a null agent_pid, an entry from another host — is false,
# which keeps the entry.
cached_agent_is_gone() { ...; }   # $1 agent_pid, $2 client, $3 entry host
```

```rust
// src/attach.rs — no host calls; pane_to_tab still supplies the `pane_ids`
// context key that src/main.rs:447-452 filters on.
pub fn run(session_name: &str, pane_to_tab: &HashMap<u32, (usize, String)>) -> bool
```

Context keys shrink to `type`, `pane_ids`, `scan_started_ms`; `pane_leaders` and
`introspection_supported` go. `emit_cached_states`' jq projection must gain
`agent_pid` **and `host`**, neither of which it extracts today (only `pane_id`,
`client`, `ts_ms`).

Three load-bearing rules:
- **Single batched pass** — for the walk. One `grep -z` (or equivalent) over the
  environ files, not a per-process shell read loop: 22 ms versus 20.3 s, because
  bash falls back to one-byte reads on unseekable `/proc` files. This does *not*
  govern the validator, where N is one session's cached entries and the safety
  rule reads per-pid; a per-entry `ps` there is correct.
- **Drop only on positive evidence.** The naive `[ -z "$out" ]` is the trap: `ps
  -o comm= -p` prints nothing both when the pid is gone (exit 1) and when `ps`
  could not run (exit 127), and a transient failure hits every cached entry in
  the same run — so reading empty output as death would not lose one row, it
  would blank the whole restore. Decide on exit status. Verified identical on
  both platforms: live 0, dead 1.
- **Normalize `comm` by basename; never use `ucomm`.** Measured on macOS
  hardware 2026-09-03: `ps -o comm=` returns `/Users/…/claude` for claude but
  bare `fish` for fish — both forms on the same OS, so no `uname`-keyed path
  strip will do. `ps -o ucomm=` returns `2.1.191` for claude, the rewritten
  process title, while on Linux `ucomm` merely aliases `comm` — so a `ucomm`
  implementation passes every test in this repo and matches nothing on macOS.
  The rule is implemented **twice**, in `find_agent_pid`
  (`scripts/zellaude-hook.sh:283-285`) and in the validator, because the attach
  script invokes the hook as a separate process (`:19`, `:240`, `:403`) and the
  two share no `source`. Two copies of one rule drift; keep them worded alike
  and carry the `2.1.191` measurement in both comments.

**Verification**
Runnable commands, and the exact strings the verifying agent must copy or match,
stay on one line however long: it reads the raw markdown, so a wrapped command
runs as two and a wrapped pattern matches nothing. Quoted log lines and
referenced flags may wrap — they are read, not executed.
- `cargo test --target x86_64-unknown-linux-gnu --features zellij-utils/vendored_curl`
- `cargo build --release` (default target `wasm32-wasip1`)
- `bash tests/attach_detection.sh`
- `bash tests/hook_mode_detection.sh`, `bash tests/install_end_to_end.sh`,
  `bash tests/install_hooks_idempotency.sh`,
  `bash tests/install_permissions_idempotency.sh` — the install suites assert the
  exact 7-element permission list, which must not shrink.
- **The macOS `comm` form is testable on Linux — stub `ps`, and bind the flag.**
  Linux `comm` is already a basename (`tests/hook_mode_detection.sh:499-501`
  says so in its own comment), so real coverage there can only confirm a no-op.
  Put **one** fake `ps` on `PATH` that reproduces the measured macOS pair by
  **branching on its own arguments**: `-o comm=` returns
  `/Users/x/.local/bin/claude`, `-o ucomm=` returns `2.1.191`. Then
  comm-plus-basename passes and a `ucomm` implementation receives a version
  string where it needs an agent name and fails. Two argument-blind fakes would
  *not* bind this: each would be handed its own canned string regardless of the
  flag asked for, so a `ucomm` implementation passes both — they would test the
  normalization, which was never in doubt, and leave the flag choice, which is
  what actually broke, uncovered. **Required at both sites.** The rule is
  implemented twice and the copies live in different suites — `find_agent_pid`
  under `tests/hook_mode_detection.sh`, the validator under
  `tests/attach_detection.sh`. Stub only one and the other copy is free to drift
  into `ucomm` or a `uname`-keyed path strip with nothing to catch it, which is
  the precise failure the three-part rule exists to prevent.
- `cargo clippy --target x86_64-unknown-linux-gnu --features zellij-utils/vendored_curl`
  must add no new warnings. The clean tree already
  emits three (`usize`→`usize` cast, manual `is_multiple_of`, collapsible `if`);
  leave them.
- **E2E** — two legs, both required.
  - **Leg A, the stall — an A/B with its control inside the run.** Two arms: the
    fixed artifact, built here, and a base artifact built from
    `git merge-base HEAD <integration branch>` — after the merge row that commit
    is this tree
    minus this change, which is the control we want. **Build the base arm in a
    throwaway `git worktree add` at that commit and remove it after.** Never
    `git checkout` it in this working directory: the row runs on the merged and
    tidied feature branch, the exact tree checkpoint 5 then reviews, and a
    checkout replaces it. Never `git stash` either — this directory is a
    worktree and the stash stack is shared with the main checkout and every
    other worktree. Run the same probe against each arm: a throwaway session
    `zellaude-e2e-$$` of **20 terminal panes** from a layout under
    `/tmp/nonblocking-attach-scan/` loading that arm's artifact by absolute
    path, then attach a client and wait ~25 s. Never copy over
    `~/.config/zellij/plugins/zellaude.wasm` — that changes the user's live
    sessions. **Observable:**
    lines matching `GetPanePid timed out|GetPaneRunningCommand timed out`
    in that run's own zellij log. Set
    `TMPDIR` per run so the log is isolated (`$TMPDIR/zellij-$(id
    -u)/zellij-log/zellij.log`) — zellij derives its log and socket dirs from
    `temp_dir()`, so the count is attributable to this session alone and the
    throwaway never appears in the user's `zellij list-sessions`. Every command
    targeting it needs the same `TMPDIR`. **Pass: the base run emits at least
    one such line and the fixed run emits zero.** Report both counts as numbers,
    not as a verdict: the pair is the feature's central evidence — the only
    direct demonstration that the freeze mechanism is gone — and a base of 1
    versus a base of 5 also says how discriminating the box was that day, before
    anyone reads a single clean run as proof. Both numbers belong in the review
    guide and the PR text. The two halves are different
    kinds of claim, and the difference is the point. `fixed == 0` is
    **structural**: there were exactly three call sites — `src/attach.rs:63` and
    `:70` plus `src/main.rs:512`, as they stood before T4 — and this change
    deletes all three, so
    with no call site there is no call, whatever the load; a nonzero result
    means an incomplete deletion or a new caller. `base >= 1` is **statistical**
    — the positive control showing the instrument fires on this box in this
    sitting. Contention threatens only the base arm, which is what the base
    assertion guards. Do not "improve" this by making the fixed side a count or
    a threshold: the arms run at different moments under different load, so
    `base >= 1` does not prove the instrument would have fired during the fixed
    run, and that gap would matter only if the fixed side were one. A base run
    of zero means the box was not discriminating that day — report inconclusive,
    never pass, and never hard-fail: that is a statement about the box, not the
    code, and hard-failing pressures whoever hits it into re-running until
    green. One re-run for a discriminating base is legitimate; a persistently
    zero base means the E2E cannot run here, which is the genuine-impossibility
    line — the user's call, routed through the planner, never the agent's.
    Measured 2026-09-03 on the unfixed build, isolated logs: 2 and 4 lines in
    two runs, e.g. `GetPanePid timed out after 5s for plugin 4 requesting pane
    Terminal(0)`. The in-run control is what makes this sound: a timeout fires
    only under contention, so a fixed count threshold measured on another day
    would not hold.

    Three things that silently void the leg, all found by
    running it: create with `zellij -s <name> -n <layout>` (`--layout` alongside
    `--session` adds a tab to an *existing* session and fails); unset `ZELLIJ`,
    `ZELLIJ_SESSION_NAME`, `ZELLIJ_PANE_ID` or zellij attaches instead of
    creating; and set an isolated `XDG_CACHE_HOME` holding a `permissions.kdl`
    granting the 7 permissions to **both arms' absolute artifact paths**, since
    grants are keyed by plugin path and the arms must sit at different paths to
    coexist (one `XDG_CACHE_HOME` per arm works too). Miss either and that arm
    waits on a permission prompt, its scan never runs, and it emits zero lines —
    which on the base arm is indistinguishable from a non-discriminating box, so
    a setup error reports as an environment result. Kill the session after.
    Loading the plugin also runs `installer::run_install()` against the real
    `~/.claude` hook config — expected and idempotent
    (`tests/install_hooks_idempotency.sh`); this change does not touch the
    installer.

    Do **not** assert on action latency. Measured: the blocked call
    sits on a `plugin-exec` thread, so zellij's action router stays responsive.
    Sampling `zellij action dump-layout` across the attach window on the unfixed
    build gave median 349 ms / p90 578 ms against a no-client floor of median
    316 ms / p90 644 ms — the attach window was *faster* at p90, and a separate
    200-sample run crossed 1 s only twice. There is no latency signal to assert
    on. Nor is the shared `/tmp/zellij-$(id -u)/` log usable: it already holds
    17k timeout lines, mostly `Action CliPipe` from unrelated causes, and the
    user's own sessions run the same unfixed plugin.
  - **Leg B, discovery.** Run the modified script against a live session here
    with real agent panes: `bash scripts/zellaude-attach.sh <session> <now_ms>`
    — run directly, the script file is `$0`, so `<session>` is `$1`; the
    `zellaude-attach` token exists only in the plugin's `bash -c` form
    (`attach::run`'s `run_command` call, `src/attach.rs:41-51` after T4).
    Pass: exits 0, emits at least one NDJSON line, and **every line whose
    `hook_event` is `SessionRestore`** — the discovery rows — carries a non-empty
    `session_id` and a `pane_id` present in that session's manifest terminal pane
    ids (`~/.cache/zellaude/runtime/<session>.manifest.json`). At least one such
    row must be present.
    **Do not assert that on every line.** `emit_cached_states` prints cached rows
    verbatim ahead of discovery, carrying whatever `hook_event` they were cached
    with — measured, 9 rows of `Notification` in one run. Rewriting `hook_event`
    to `SessionRestore` is the *plugin's* step (`src/main.rs:470`), not the
    script's, so asserting it against script stdout tests the wrong side of the
    boundary.
    **Cached rows carry a positive assertion of their own:** every emitted row
    with a non-null `agent_pid` and a local `host` must name a live process —
    one `ps` per row. This is **racy and must not hard-fail on a single miss**:
    the cache is read at `:19` and the check runs after the script exits, so an
    agent exiting in that window makes a correctly-kept row name a dead pid.
    Follow the pattern used for the non-discriminating base arm and the
    agent-less session — one miss is **inconclusive, never pass, never
    hard-fail**; the *same* row missing on a re-run, or every row missing at
    once, is real and fails. That is the validator's keep path exercised against real
    pids and a real cache, which nothing else covers: the `ps` stubs test the
    rule in isolation against a fake `ps`. The drop path is not asserted here;
    proving it live means planting a stale entry, which is a fixture problem.
    **Bound the walk, not the total.** A total bound would measure the hook
    subprocess cost this feature does not touch: measured 2026-09-03 against a
    live 9-agent session with 765 processes, 5.77 s total, of which the walk is
    39 ms — consistent with the 22 ms for 528 pinned above — the rest being one
    hook `--inspect` per discovered pane (~335 ms each, `:271`, `:434`) plus one
    `--restore` (~136 ms, `:19`), all of which scales with live agents and
    predates this change. So bound the walk in a *second* invocation with
    `ZELLAUDE_ATTACH_HOOK` pointed at a no-op stub, which removes the subprocess
    cost and leaves the `/proc` pass.
    **Bound it with three terms, one per cost the run actually carries:**
    `elapsed < 500 ms + 1 ms x processes_seen + 200 ms x candidates_seen`.
    Record all three counts in the artifacts so the figure can be re-derived.
    Each term is calibrated on a measurement, and conflating them is what made
    the first version of this bound fail correct code:
    - **1 ms x processes** guards the batched-pass rule. Measured 0.051
      ms/process (39 ms / 765); the forbidden read loop costs 38.45 ms/process
      (20.3 s / 528). ~20x headroom, and a read loop overshoots this term alone
      by ~38x.
    - **200 ms x candidates** covers the per-candidate `discover_*` work that
      the stub *forces to completion* — see the note below. Measured ~81 ms per
      candidate, so ~2.5x headroom. This term scales with agents, not with
      `/proc`, which is why it cannot be folded into the per-process term.
    - **500 ms fixed** covers `--restore` (~136 ms) and process startup, which
      do not scale at all. Without it a per-process bound false-fails on a small
      box, where a fixed cost dominates.
    A flat per-process bound was wrong twice over: at 1 ms/process it gives
    **0.9x** margin against the real composition (817 ms measured over 755
    processes with 9 candidates) and fails correct code, which is what T7 caught;
    and raised to a flat 5 ms/process it false-fails at ~100 processes while
    still passing there for the wrong reason. The three-term form gives ~3.7x
    margin on the measured run and still catches a read loop by ~10x.
    Without this bound nothing anywhere asserts the batched-pass rule:
    `tests/attach_detection.sh` runs against a fake `PROC_ROOT` of a handful of
    processes, where a read loop is invisible, and recording elapsed time in
    artifacts makes a regression visible but catches nothing.

    **Only the 1 ms/process term is frozen.** It is the sole enforcement anywhere
    of the batched-pass rule, so it must never be raised — raising it is how that
    rule gets quietly disabled. The 500 ms fixed and 200 ms/candidate terms
    bracket costs this feature does not own and may be recalibrated on evidence.
    This replaces the earlier blanket "do not correct the bound", which was
    ambiguous across three numbers.
    **Record the decomposition, not just the total** — walk elapsed, processes
    seen, candidates seen, stub spawns. A breach must be **attributed to a term**
    before it is treated as a walk regression: a false-fail from the candidate
    term looks identical to a real one from the process term, and the reflex fix
    for an unattributed breach is to raise a number.
    Isolating the walk instead — stubbing against a session name no process
    carries, giving the full `/proc` pass with zero candidates — was considered
    and rejected: with no candidates there is no `--inspect`, so it loses the
    marker proving the walk ran and the `chmod` trap returns. Candidates are the
    price of proving the walk happened.
    **The stub must be executable, and the run must prove it walked.**
    `[ -x "$HOOK_PATH" ] || exit 0` is line 15, so a stub without `chmod +x`
    makes the script exit 0 immediately: the bound passes trivially, no rows are
    emitted, and the correctness assertions live on the *real-hook* invocation
    so they still pass — a green leg that measured nothing. Have the stub append
    its invocations to a file and require at least one **`--inspect`**, which
    proves the walk ran and found a pane's agent. A `--restore` marker will not
    do: `:19` runs before the `uname` gate, so it fires even when the walk never
    does. Leg B's preflight guarantees a live agent pane, so requiring one
    `--inspect` is safe.
    Note the stubbed run does *more* per-candidate work than the real one — every
    `discover_*` fails, so the loop never breaks early and every candidate gets
    its own environ, cwd and fd reads. That is conservative in the right
    direction: it measures more than the walk, so it cannot hide a slow one. Do
    not "correct" the bound after noticing the extra work.
    Read-only — the hook calls use `--inspect` and `--restore`, neither writes.
  - **Preflight, Leg B:** the target session must hold at least one live agent
    pane. If it holds none, the leg cannot prove discovery works and must report
    a **no-go, not a failure** — the same shape as Leg A's non-discriminating
    base arm. An environmental absence of agents reaching the planner as a defect
    wastes a routing cycle and invites someone to "fix" working code.
  - **Preflight, Leg A:** no scarce resource — no GPU, no quota, 20 shells against
    ~1.99 TB free RAM. Two real go conditions: `zellij` on PATH, throwaway
    session name unused (`zellij list-sessions`). Not met → report, do not make
    room.
  - **Artifacts:** `/tmp/nonblocking-attach-scan/` — layout, isolated
    `XDG_CACHE_HOME`, samples, timings. Never in the repo, never staged. Leg A
    also leaves state outside `/tmp`: the plugin writes
    `~/.cache/zellaude/runtime/<session>.manifest.json` into the user's real
    cache, and it survives session deletion. Delete the throwaway session and
    that manifest on the way out; verified both are needed.

**Decisions**
- **Pane → agent selection:** pick agent processes from each pane's process set;
  do not rebuild a pane leader. The alternative keeps `foreground_pid` and the
  parent climb but must infer which process Zellij spawned. Measured here, the
  seed's rule ("parent outside the pane") left **3 of 13 panes with two or more
  candidates**: reparented transients look like leaders. That inference fails
  silently — a wrong pane→session binding shows the wrong row, no error. Direct
  selection infers nothing, and its own ambiguity (a nested `claude -p`) is
  already caught by `discover_claude`'s registry check (`kind=interactive`,
  `entrypoint=cli`, `:331-336`), tested at `:255-266`. Cost: the case at
  `tests/attach_detection.sh:195-212` proves the parent climb, and under this
  design that scenario stops being one the code can fail.
- **Candidate order:** ascending pid, for determinism. The registry check does
  the real work; order only fixes which candidate is tried first.
- **One validator for both platforms; discovery is the only Linux-only part.**
  Validation of a cached entry asks "is this still true?", which needs only the
  entry's own `agent_pid` and `ps` — identical on Linux and macOS. Discovery
  asks "what is running in each pane?", which needs another process's
  environment: `/proc` on Linux, blocked by SIP on macOS. So the split falls on
  a real capability boundary, not on an OS preference: one shared validator,
  plus the `/proc` walk as a Linux-only enrichment behind the existing `uname`
  gate (`scripts/zellaude-attach.sh:141`). This is what the user asked for —
  common logic first, per-OS only where a capability genuinely does not exist.
- **Liveness is the sole validator; the walk-derived pane check is not layered
  on top of it.** Composing them is not merely redundant, it is wrong. On a
  renamed Linux session the walk runs and matches nothing, because
  `ZELLIJ_SESSION_NAME` in `/proc` is the launch-time value
  (`src/state.rs:202-205`); a walk-derived filter would then drop an entry whose
  pid is plainly alive and plainly `claude`. That is the inverse of the hazard
  the earlier draft guarded — an empty walk wrongly *applying* a filter rather
  than wrongly skipping one. Liveness gets that case right. `PANE_SCAN_RAN` and
  the "no scan ⇒ no filter" rule both disappear with it: the validator no longer
  depends on whether the walk ran. The residual gap is the **null-`agent_pid`
  population**, which liveness keeps unconditionally: every macOS entry until
  the basename fix propagates through a hook event, entries predating commit
  7c87365 that lack the field, and any failed `$PPID` climb. If that is ever
  covered, cover it as a null-only fallback that keeps the empty-walk guard —
  never as a second filter composed over the first.
- **Pid reuse is accepted, not defended against.** `cached_agent_is_gone` pairs
  the pid with a `comm` match, which catches a recycled pid running something
  else but not a recycled pid that happens to be another agent. Left as is: the
  failure is a false *keep*, which is exactly the behaviour of dropping the
  check altogether, so it introduces no new failure mode. Pairing with a start
  time would defeat it — Linux discovery already does that for the registry
  match (`scripts/zellaude-attach.sh:325-336`) — but the macOS equivalent (`ps
  -o lstart=`) is unverified, and buying a rare collision with an unverified
  second platform assumption is the wrong trade in a validator whose whole
  licence to ship is that it cannot do harm.
- **Foreign-host cache entries are never validated.** The cache is shared on
  shared homes — that is why `agent_pid` carries a `host` beside it
  (`scripts/zellaude-hook.sh:275-278`) — and nothing upstream filters on it:
  `restore_cached_states` selects on `zellij_session`, `is_subagent` and
  `session_id` only (`:202-205`), and the attach script never reads `host` at
  all. So a foreign pod's entry can reach the validator, where `ps` would test a
  remote pid against the local process table: a live local pid keeps a dead
  remote entry, a dead one drops a live remote entry. The rule follows from
  "drop only on positive evidence" — a foreign pid is evidence about nothing
  local, so validate only when the entry's `host` equals the local hostname and
  keep otherwise. `host` joins `agent_pid` in the jq projection. Two edges to
  pin, both of which reopen the false drop if missed. **Never phrase it as "drop
  when host differs"** — that inversion is the same family as `[ -z "$out" ]`
  and reads as the natural implementation. And **require both sides non-empty**:
  `AGENT_HOST=$(hostname 2>/dev/null) || AGENT_HOST=""`
  (`scripts/zellaude-hook.sh:292`) lets the cache hold a null host, and the same
  `hostname` call can fail at validate time — if both normalize to empty, empty
  equals empty, the entry is treated as local, and a hostless entry is validated
  against a process table it never came from. A null host must keep, which is
  also what lets entries predating commit 7c87365 survive. This incidentally
  closes the container case for free: an agent whose pid was recorded inside a
  different pid namespace carries a different hostname, so it lands in "not ours
  to judge".
- **The validator assumes `comm` is stable for the life of the process.** This
  is the last piece of positive evidence that could be wrong, so it is written
  down rather than left implicit: if a process's reported `comm` changed after
  the cache recorded its client, a live agent would be judged stale — a false
  drop. The macOS run is evidence for stability rather than against: `ps -o
  comm=` returned `/Users/…/claude` while `ucomm` returned the rewritten title
  `2.1.191`, so the title rewrite Claude Code performs lands in `ucomm` and
  leaves `comm` alone. That is one more reason the rule pins `comm` and forbids
  `ucomm`.
- **macOS keeps discovery and gains the freeze fix.** It recovers *part* of what
  this change removes, not parity: pane binding today, liveness after. The
  `agent_pid` fix is not optional scope — without it macOS entries are null and
  the shared validator never fires there. It also repairs a live bug:
  `agent_pid` exists so external tools get exact kill/inspect targets (commit
  7c87365, `scripts/zellaude-hook.sh:275-278`), and `zellmv` has been reading a
  null target on macOS. Keeping the host calls on macOS instead was rejected —
  the plugin could learn the host OS at runtime (`uname` is one async
  `run_command` round trip, so the missing compile-time `cfg!` is not the
  obstacle), but that preserves the freeze on a platform that suffers it today.
- **What a kept entry costs, and when it clears.** The validator drops an entry
  whose `agent_pid` is demonstrably dead, so the abrupt-kill case is now caught
  at attach time. What it keeps by design — null `agent_pid`, foreign host, or a
  live pid — can still leave a row on the bar, and that row is **not** bounded by
  the 12 h window: `cache_max_age_ms` (`scripts/zellaude-attach.sh:93`) gates
  only whether an entry is seeded at attach time, and nothing ages a merged row
  out — `cleanup_stale_sessions` (`src/main.rs:1941-1955`) only demotes
  `Done`/`AgentDone` to `Idle` after `DONE_TIMEOUT`, never removes. It clears
  when the pane closes: `remove_dead_panes` (`src/main.rs:1936-1939`) drops rows
  whose pane has left `pane_to_tab` — self-correcting rather than immediate,
  since it runs only through `rebuild_pane_map` (`src/main.rs:1919-1925`) from
  `TabUpdate` (`:178`) and `PaneUpdate` (`:187`), the same events required to see
  the row at all. A clean agent exit also clears the cache entry, but only when
  it still names the ending session **and** the event is not older than the entry
  (`scripts/zellaude-hook.sh:987-997`).
- **Argv `$2` removed, not repurposed:** its two readers are gone — the drive
  loop is replaced by the walk, and the cross-check it fed is replaced by the
  pid-liveness validator, which reads the cached entry rather than argv. The
  plugin already filters discovered panes twice — `allowed_panes` at
  `src/main.rs:477-481`, `pane_to_tab` at `:504-506` — so a script-side
  allowlist carries no correctness role, and such a parameter drifts. Reading
  the pane set from the exported manifest was rejected: `src/manifest.rs:7`
  documents "Never carries `agent_pid`" as a deliberate invariant, asserted at
  `:239-241`, so the manifest can supply the pane set but never the pane→process
  map; and the plugin holds `pane_to_tab` in memory while writing that file
  asynchronously, so it is never fresher than argv.
- **Post-scan leader re-check dropped, no replacement:** verifying the leader is
  the very host call being deleted. Dropping it also fixes a latent bug — with
  introspection supported and no leader surviving the loop,
  `src/main.rs:507-517` discarded every payload, cached entries included. The
  merge keeps the other guards: `pane_to_tab` membership, `allowed_panes`, the
  `scan_started_ms` freshness window, and `event_handler`'s tombstone and
  ordering checks.
- **`client_for_command` deleted, no functional loss:** the field it fed was
  only an allowlist gate; the script has always dispatched on
  `$PROC_ROOT/<pid>/comm`, matching `codex` and `claude` exactly. A pre-exec
  `better-codex` never matched that case anyway, so the approved design at
  `docs/superpowers/specs/2026-07-31-better-codex-support-design.md` is marked
  superseded, not reimplemented.
- **Version floor and permissions unchanged:** `zellij-tile >= 0.44.3`, the 0.44
  install gate and the 7-element `REQUIRED_PERMISSIONS` array all stay.
  `ReadApplicationState` is still needed for `PaneManifest`/`TabInfo`, and
  `get_zellij_version` still has a live caller in `split_three`. Deleting the
  0.44-only host calls is no licence to relax any of them.
