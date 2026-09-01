## Plan: Capture Claude Launch Environment

Record each agent's launch environment in the per-pane state cache so relaunch tools (`zellmv`) can
reproduce it instead of silently reaching the default Anthropic endpoint. The hook gains two payload
fields: `launch_env`, read from the agent's own `/proc/<agent_pid>/environ` against one client-agnostic
allowlist of exact variable names, and `current_effort_level`, resolved through the precedence the hook
already uses. Secret-named variables record a marker, not their value, with one literal escape. A settings file may
extend the allowlist by adding names; the two tiers and the escape set are frozen in code.

**Context**
Env-prefixed launches (`ANTHROPIC_BASE_URL=… ANTHROPIC_AUTH_TOKEN=… claude --effort xhigh`, an abbr for
per-session backends) are invisible to the cache: it proves identity (`session_id`, `cwd`, `agent_pid`)
but records nothing about how the agent started, so any relaunch reaches the default endpoint. The capture half is invisible on its own — its
value arrives with the replay half, `feature/zellmv-faithful-resume` in my-terminal-setup, and this is
the producer side of that contract. The user-extensible allowlist added by revision IS user-visible,
which is why this feature owes README documentation (T13) that the original scope did not.
Branch `feature/capture-claude-launch-env`; the merge task merges in `develop`.
Mode: ask

**Approach**
- Phase 1 (T1–T2): add the allowlist and the environ reader to `scripts/zellaude-hook.sh`; extract the
  effort-level precedence now buried inside `detect_claude_rainbow`.
- Phase 2 (T3): wire both fields into `PAYLOAD`, guard them in the `persist_root_state` merge, strip
  `launch_env` from the `--restore` output.
- Phase 3 (T4): fixture-driven tests over both capture paths, the secret policy, and the merge guard.
- Phase 4 (T9–T11, added by revision): extend the allowlist with the residual codex names, then make it
  user-extensible from `zellaude.json` under the frozen-tier rule, with tests for both. T12 records two
  planner-ordered amendments that landed under chat orders while impl1 was mid-run.
- Phase 5 (T13): document the new settings key in the README — the expansion gives this feature a
  user-facing surface it did not have.
- T3 needs T1 and T2; T4 needs T3; T9 needs T4; T10 needs T9; T11 and T13 need T10. Every row above edits
  one of three files — the hook, its test suite, and the README — and is strictly sequential on one
  agent.

**Relevant files**
- `scripts/zellaude-hook.sh` — the whole change: allowlist + reader near `find_agent_pid`, the extracted
  effort resolver, the `PAYLOAD` jq block, the merge jq in `persist_root_state`, the projection in
  `restore_cached_states`.
- `tests/hook_mode_detection.sh` — the harness already runs the real hook with a fake `zellij` capturing
  `PAYLOAD`; new payload-field cases belong beside it.
- `README.md` — the user-facing surface the expansion creates. The allowlist is a hand-edited
  `zellaude.json` key, not a menu toggle, so it documents alongside **Custom states** and **Session
  templates**, not in the Settings table of bar-menu options.
- `tests/attach_detection.sh` — reference only. Source of the `write_environ` fixture idiom (`:21-27`)
  and the `ZELLAUDE_PROC_ROOT` seam (`:7,:122`).
- `scripts/zellaude-attach.sh` — reference only. `proc_env_value` (`:21-25`) is the in-repo precedent for
  reading `/proc` environ; `PROC_ROOT` (`:12`) is the seam being mirrored.
- `src/installer.rs` — reference only, but it consumes this shell file: `:8` pulls it in with
  `include_str!` and stamps a version tag after the shebang.
- `src/manifest.rs` — reference only, and the reason edits near the top of the hook are risky:
  `state_cache_key_shell_fn` (`:100-105`) slices `state_cache_key` out of that embedded string by
  literal search for `"\nstate_cache_key() {"` and its closing `"\n}"`. The new code lands near
  `find_agent_pid`, far from it, but the coupling is invisible from the shell file alone.

**Naming & signatures**

```bash
# Exact names, client-agnostic, two tiers. No prefix matching and no denylist: an injected
# variable is excluded by not appearing here.
LAUNCH_ENV_CONFIG_NAMES="ANTHROPIC_BASE_URL ANTHROPIC_MODEL CLAUDE_CODE_USE_BEDROCK \
CLAUDE_CODE_USE_VERTEX CLAUDE_CODE_EFFORT_LEVEL ZELLAUDE_CLAUDE_MODE CODEX_HOME \
CODEX_SQLITE_HOME OPENAI_BASE_URL"                             # value captured verbatim
LAUNCH_ENV_SECRET_NAMES="ANTHROPIC_AUTH_TOKEN ANTHROPIC_API_KEY CODEX_API_KEY OPENAI_API_KEY"
LAUNCH_ENV_SAFE_SECRET_VALUE="local"   # literal set, never a pattern — see Decisions
LAUNCH_ENV_REDACTED_MARKER="<set>"     # records that a secret was present, not its value

PROC_ROOT=${ZELLAUDE_PROC_ROOT:-/proc}  # mirrors zellaude-attach.sh:12 so tests can supply a fixture

# Emits the launch_env object as compact JSON, or nothing when the environ cannot be read.
# Reads NUL-separated entries so a value containing a newline survives — which is why
# proc_env_value's `tr '\0' '\n' | sed` cannot be reused.
read_launch_env() { : "$1"; }          # $1 = agent pid

# The single resolution of the agent's current effort, used by BOTH the payload field and
# detect_claude_rainbow. Precedence is the repo's own: stdin .effort.level, then CLAUDE_EFFORT.
resolve_effort_level() { :; }   # downcases BOTH sources — see Decisions

# Built-in lists merged with the user's, per the frozen-tier rule in Decisions.
# A missing, unreadable or malformed settings file yields the built-ins unchanged.
merged_launch_env_names() { :; }
```

New payload fields:

```
launch_env            object | null   # null = not captured; {} = captured, nothing matched
current_effort_level  string | null   # effort as of this event's ts_ms, NOT launch-time;
                                      # always null for CLIENT=codex — see Decisions
```

- `current_effort_level` departs from the seed's `effort_level` deliberately. Beside `launch_env` a bare
  `effort_level` reads as launch state, and the contract already carries `launch_effort_level`
  (`zellaude-attach.sh:376`) and `launch_env.CLAUDE_CODE_EFFORT_LEVEL`, both launch-time. The qualifier
  separates the three. Free to rename — the consumer does not exist yet.

**Verification**
- `bash -n scripts/zellaude-hook.sh`; `shellcheck` on it if available (absent from CI, so not a gate).
- `tests/hook_mode_detection.sh` and `tests/attach_detection.sh` must stay green — they cover every
  existing payload field and the `--restore` path being narrowed.
- `cargo build --release --target wasm32-wasip1`, and
  `cargo test --target x86_64-unknown-linux-gnu --features zellij-utils/vendored_curl` — the whole
  suite across ten binaries, exit 0. The claim is the exit code, not a count: a pinned number rots
  silently with every test added, and a wrong one invites the next reader to think something
  regressed. (175 tests when last run.) An earlier draft said `cargo test` could not run here (host target needing absent OpenSSL);
  `vendored_curl` removes that, so it is an executed check, not a documented gap.
- The Rust coupling has a real test, not the `grep` fallback an earlier draft specified:
  `manifest::tests::extracted_shell_function_matches_the_hook_key_scheme` exercises the literal-search
  slice `src/manifest.rs` takes out of the embedded hook, and it runs in the unit-test target of the
  command above — no `[lib]` in Cargo.toml, so the unit tests compile into the bin target.
- **E2E** (required): a real `claude`, a real Zellij session and the **branch's** hook write a real
  cache file — in a staged home, so nothing the user is running is touched.
  1. `export ZELLAUDE_E2E=/tmp/capture-claude-launch-env` and make `$ZELLAUDE_E2E/{home,runtime}` mode
     `700`. `runtime` must be `700` and user-owned or `state_cache_dir` will reject it.
  2. `ZELLAUDE_INSTALL_HOME=$ZELLAUDE_E2E/home ./install.sh --no-permissions` — the repo's own isolation
     override (README: "running an isolated test"). It installs **this branch's** hook and registers it
     in the staged Claude settings. The live `~/.config/zellij/plugins/zellaude-hook.sh` every agent on
     this box uses is never written.
  3. Stage `$ZELLAUDE_E2E/home/.config/zellij/plugins/zellaude.json` carrying (a) `ANTHROPIC_CUSTOM_HEADERS`
     added to the config tier — a real vendor variable deliberately NOT in the built-in list, which the
     launch line also sets to `e2e-extends` — and (b) an attempt to demote `ANTHROPIC_API_KEY` to the
     config tier. (a) proves the merge extends; (b) proves the frozen-tier rule holds under a
     *hostile* config, which is the half the design was chosen for — and it needs no new assertion,
     because the existing `ANTHROPIC_API_KEY == "<set>"` check becomes that proof.
     Stage it BEFORE launching the agent: the hook reads the settings file at event time, so a
     config staged afterwards is not in the merged allowlist when `SessionStart` fires and the
     `ANTHROPIC_CUSTOM_HEADERS` assertion loses a race it should never be in.
  4. Derive the `env -u` list by parsing `LAUNCH_ENV_*_NAMES` out of the hook rather than transcribing
     it — that makes the key-set invariant self-enforcing instead of merely stated. Control the
     environment at **session** start, not pane start: `zellij action new-pane` spawns from the zellij
     server, so a pane started that way inherits the server's environment, not the controlled one, and
     every assertion below depends on that control.
     Start an isolated Zellij session named `zellaude-e2e-launchenv` (preflight: it must not already
     exist) and run one pane under a **controlled** environment — the environment is what this feature
     measures, so it cannot be left ambient. Unset every allowlisted name, then set only the names the
     case needs, the idiom `tests/hook_mode_detection.sh:36-38` already uses:
     `env -u` **every** allowlisted name the case does not set, plus `CLAUDE_EFFORT`. Standing rule,
     stated so it survives the next row that extends the list: *every allowlisted name is either set by
     the E2E or unset by it — none left ambient.* A count would go stale; this does not.
     `HOME=$ZELLAUDE_E2E/home XDG_RUNTIME_DIR=$ZELLAUDE_E2E/runtime`
     `ANTHROPIC_BASE_URL=http://127.0.0.1:9/ ANTHROPIC_AUTH_TOKEN=local`
     `ANTHROPIC_API_KEY=e2e-not-a-real-key CLAUDE_CODE_EFFORT_LEVEL=high`
     `ANTHROPIC_CUSTOM_HEADERS=e2e-extends claude --effort high -p hi`.
     Not `env -i`: that would also wipe `ZELLIJ_SESSION_NAME` and `ZELLIJ_PANE_ID`, which the hook
     requires and exits on (`:263-264`). `HOME` must be the staged one — the installer registers the hook as the
     literal `${HOME}/.config/zellij/plugins/zellaude-hook.sh` (`install-hooks.sh:15`), so this is what
     makes Claude Code invoke the branch's script rather than the live one. The unroutable base URL is
     deliberate: `SessionStart` fires, the request dies immediately, **no API quota is spent**.
  5. Read `$ZELLAUDE_E2E/runtime/zellaude-$(id -u)/zellaude-e2e-launchenv.<pane>.json` — **snapshot it
     while the session is still alive**. `SessionEnd` deletes the pane's own cache entry (correct,
     pre-existing behavior), so reading after the run races that deletion.
     Staging
     `XDG_RUNTIME_DIR` is what makes this path deterministic: `state_cache_dir` (`:48-78`) only uses it
     when it is absolute and passes `is_owned_private_parent`, and otherwise silently falls back to
     `$HOME/.cache/zellaude/runtime`. Do not assume the branch — if the file is not there, look in the
     fallback before reporting.
  - **Pass condition**, observable, no judgement:
    `.launch_env.ANTHROPIC_BASE_URL == "http://127.0.0.1:9/"`;
    `.launch_env.ANTHROPIC_AUTH_TOKEN == "local"` (the safe-value escape);
    `.launch_env.ANTHROPIC_API_KEY == "<set>"` (the redaction the escape is an escape *from* — the
    headline security decision, otherwise never crossed end to end);
    `.launch_env.CLAUDE_CODE_EFFORT_LEVEL == "high"` (config-tier capture);
    `.launch_env.ANTHROPIC_CUSTOM_HEADERS == "e2e-extends"` (the merge extends — a name captured ONLY
    because the staged config added it);
    `(.launch_env | keys) == ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL",`
    `"ANTHROPIC_CUSTOM_HEADERS", "CLAUDE_CODE_EFFORT_LEVEL"]` — the whole key set, not a spot check.
    **Invariant, stated so a missing edit surfaces here rather than at run time: the expected key set IS
    exactly the set of allowlisted names step 4 sets.** Change either and the other is wrong; a literal
    list alone cannot say that. An exact-name allowlist cannot
    leak an injected variable by construction, so asserting one is absent proves nothing; asserting the
    set catches the failure that *is* possible, a reader capturing more than it should. This strictness
    is only sound because step 4 controls the environment.
    Finally `grep -q 'e2e-not-a-real-key' <cache file>` must **fail** — direct evidence that a value
    which reached `/proc` did not reach disk, which is the claim a reader of this feature most wants.
  - `.current_effort_level == null` on this run, and that is **correct, not a failure**. Note what this
    assertion can and cannot do: it documents the lifecycle case and passes identically whether the
    field works or is permanently dead, so it is not the field's coverage. The field's LOGIC is covered
    by an executed test — T4 asserts `"high"` from a stdin `effort` payload — and the VENDOR half (that
    a real claude populates `.effort` at all) by T14's recorded-payload fixture. Measured on
    claude 2.1.252, and the BEHAVIOR confirmed verbatim in that binary's hook-input schema: `effort` is
    `.optional()`, and the vendor's own text reads "Present for hooks that fire within a tool-use
    context (PreToolUse, PostToolUse, Stop, SubagentStop, etc.) ... absent for session-lifecycle hooks",
    with `CLAUDE_EFFORT` tracking it exactly. `claude -p hi` fires only lifecycle events, so both
    sources are legitimately empty.
    Separately and with a narrower warrant: `effort` sits in the BASE payload object rather than being a
    field tool events add — verified structurally in **2.1.251** only, by co-location with `session_id`,
    `transcript_path`, `cwd`, `prompt_id`, `permission_mode`, `agent_id` and `agent_type` in one object
    literal. That probe returns nothing against 2.1.252, which is evidence the probe does not survive a
    bundler rebuild, NOT evidence the field moved. Nothing in this design rests on the structural claim;
    the behavioral one above is what the field's semantics follow from.
    The field is therefore null only until a session's first tool call, after which the null-keeps-
    `$previous` rule carries the value across the lifecycle events that supply nothing. Verified end to
    end by feeding the REAL captured vendor `PreToolUse` payload into the unmodified hook: in
    `effort={"level":"high"}`, out `current_effort_level = "high"`.
    To exercise that path the run needs a tool call, which an unroutable `ANTHROPIC_BASE_URL` cannot
    produce — the no-quota property and the tool event are in direct conflict. Resolve it with a local
    mock endpoint returning an SSE `tool_use` block, never by spending real quota.
    (An earlier draft asserted `== "high"` here, on the mistaken premise that `CLAUDE_EFFORT` reaches
    every process claude spawns. That came from probing a Bash-TOOL subprocess and generalising to a
    HOOK subprocess — different spawn paths, and the tool one is precisely the context where the vendor
    does populate it.)
  - **Fallback** if `claude` will not start unattended under a staged home — report before using it:
    keep the real Zellij session and the real branch hook, substitute a process named `claude` with a
    controlled environ. This still crosses the environ read, allowlist, payload build and cache write;
    only the vendor binary is stubbed. Say so in the verification report.
  - **Preflight**: no GPU, no paid quota, negligible disk. Confirm `claude` and `zellij` are on `PATH`.
    Building the plugin is the slow part; `./install.sh` does it.
  - **Teardown, required**: kill the isolated Zellij session. Nothing else needs undoing — the staged
    home is the whole point, and no live file was written.
  - **Artifacts:** `/tmp/capture-claude-launch-env/…` — staged home, cache file, payload. Never staged
    into git.

**Decisions**
- **Value policy — fail closed, one literal escape:** config-tier names are written verbatim;
  secret-tier names as `<set>`, unless the value is exactly `local`, which is written verbatim. Chosen
  over capturing everything because the vendor keeps adding variables and this enumeration was already
  wrong twice in one session. The escape lets local-proxy sessions replay with no re-source contract
  while a real token never reaches disk. The safe-value set is a **literal set, never a regex** — a
  pattern would eventually match a real credential, the exact failure the tier exists to prevent.
- **Exact names, not prefixes — so there is no denylist:** the seed specified prefix capture minus a
  denylist of injected variables, which must stay exhaustive forever; its own list already missed
  `CLAUDE_CODE_BRIDGE_SESSION_ID`. Exact names exclude every injected variable by construction.
  Recorded because the seed says otherwise: probing live showed the parent shell itself carrying
  `CLAUDE_CODE_AUTO_CONNECT_IDE`, so the seed's macOS-only denylist scope was wrong on Linux too — any
  return to prefix capture must apply it on both paths.
- **What earns a place on the list:** launch-time knobs that change what a relaunch gets — backend,
  auth, model, or agent behavior. That is why `CLAUDE_CODE_EFFORT_LEVEL` and this repo's own
  `ZELLAUDE_CLAUDE_MODE` sit beside the routing names; the latter is read at `zellaude-hook.sh:670,704`
  and pulled from the target's environ at `zellaude-attach.sh:359` for this very question, so omitting it
  would replay `ZELLAUDE_CLAUDE_MODE=ultracode claude` without ultracode. Accepted cost: with 60+ vendor
  variables the list is curated, and a name outside the criterion is not captured until added. Known
  gap: a Bedrock replay also needs `AWS_REGION`, outside both vendor prefixes and out of scope.
- **One list for both clients:** the allowlist does not branch on `CLIENT`. `CODEX_HOME` selects which
  `config.toml` codex reads, and that file holds its `model_providers` base URL, so capturing the name
  reproduces codex routing as `ANTHROPIC_BASE_URL` reproduces Claude's — `zellaude-attach.sh:160`
  already reads it for that reason.
- **Null must not overwrite a capture:** `persist_root_state` rebuilds from `$current` and patches back
  only `rainbow_*`, so a `null` in either new field erases a good one. On a matching `session_id`, a null `$current` keeps `$previous`
  — **independent of `hook_event`**. `rainbow_name`'s version of this rule excludes `SessionStart`,
  and that carve-out must NOT be copied: for `rainbow_name` a `SessionStart` genuinely is the
  authoritative new signal, but a launch environ is fixed at exec, so within one `session_id` there is
  no newer, better `launch_env` — a null is only ever a failed read. (An earlier draft said "follow the
  rule `rainbow_name` already uses"; that shorthand imported a carve-out contradicting the very reason
  stated here, and a second `SessionStart` with a failed read would have wiped a good capture.) It
  matters most for `launch_env`: a launch environ is fixed at exec, so a later null is always
  stale-wrong rather than a legitimate update, and `null` means "not captured", which must not become a
  lie about a pane captured moments earlier. Contradicts the seed's claim that the merge already keeps
  the fields; it does not.
- **Effort precedence resolved once:** `.effort.level` from stdin, then `CLAUDE_EFFORT` — the precedence
  the repo already used, extracted from `detect_claude_rainbow` into `resolve_effort_level` and shared
  by both callers. It had sat behind early returns inside that function, out of scope when `PAYLOAD` is
  built, so resolving it a second time there would have let one process answer the same question two
  ways. This makes the change wider than the seed's footprint implied. The env fallback is
  load-bearing: `.effort` does not ride every event type, so a stdin-only value would arrive `null` on `SessionStart` — destructive per above.
- **`current_effort_level` is null for codex:** `resolve_effort_level` reads `.effort.level` (a Claude
  hook field) and `CLAUDE_EFFORT` (a Claude env var); codex effort lives in the transcript's
  `turn_context`, which is why `detect_codex_rainbow` is a separate path. On a codex pane the resolver
  is reading the wrong instrument, and since Claude Code exports `CLAUDE_EFFORT` to its children, a
  codex pane launched from such a shell would record another agent's effort — telling the consumer to
  replay a flag that was never codex's. Null is honest; a value there is confidently wrong. Subagents
  are deliberately not guarded: the parent's effort is genuinely theirs, and `persist_root_state`
  returns early for them, so it never reaches the cache.
- **Effort values are normalized:** `resolve_effort_level` downcases both sources. The hook already
  normalizes effort everywhere else — `.effort.level`, `.launch_effort_level`, the codex transcript
  efforts, and `CLAUDE_CODE_EFFORT_LEVEL` via `tr` one line away — leaving `CLAUDE_EFFORT` the only
  unnormalized source, which is a latent inconsistency rather than a decision. Accepted consequence:
  an uppercase `CLAUDE_EFFORT` now hits `detect_claude_rainbow`'s "explicit non-xhigh effort" guard
  instead of falling through to transcript detection. That fallthrough was accidental; the repo applies
  that guard case-insensitively at every other source.
- **`null` vs `{}`:** `null` means the environ could not be read; `{}` means it was read and nothing
  matched. Free, and it tells the consumer whether to trust the absence.
- **Not captured means not captured:** one branch covers `/proc` absent (macOS), the read refused, and
  no `agent_pid`. The hook never substitutes its own inherited environment — under `--inspect` that env
  belongs to `zellaude-attach.sh`, not the agent, so `--inspect` emits `launch_env: null`. That path
  runs outside the agent's process tree, and `INSPECT_ONLY` returns at `:905` before `persist_root_state`
  at `:910`, so it feeds plugin memory only.
- **Stripped from `--restore`:** `restore_cached_states` emits each cached object whole, which
  `zellaude-attach.sh:19` captures into a shell variable and the plugin parses at `src/main.rs:427-430`.
  `del(.launch_env)` removes three transit surfaces at no cost — the plugin ignores the key
  (`HookPayload` has no `deny_unknown_fields`) and the consumer reads the cache files directly.
- **`ZELLAUDE_PROC_ROOT` in the hook:** mirrors `zellaude-attach.sh:12` so the capture path is testable
  from a fixture tree instead of a live agent. `/proc/<pid>/environ` is fixed at exec, so the per-event
  read recomputes an identical result; the per-event cost is now TWO file reads plus their `jq`
  invocations — the environ and the settings file, which moves from the rare `PermissionRequest` branch
  to every payload-building event. Accepted rather than cached: the hook already runs many `jq`
  invocations per event, and a cache-validity scheme would have to solve staleness against a file the
  user edits by hand.
- **Configurable by extension only; the tiers are frozen:** a settings file may ADD names, saying which
  list each addition joins. It can never re-tier a predefined name, and it can never touch the escape
  set. Both halves of the boundary are code-level facts, fixed at runtime. The escape set matters as
  much as the tiers: a config that could add a safe value would write verbatim secrets for every
  secret-tier name without touching a tier at all — the same boundary through a cheaper door. The
  PLAN's own reasoning settles it: if a pattern is too risky because it would eventually match a real
  credential, a user-supplied value carries that risk with less review.
- **A bad settings file is ignored:** missing, unreadable, or malformed yields the built-in lists
  unchanged — fail-safe. Never fail-open (silently capturing more) and never fail-dead (capturing
  nothing). The repo answers this the same way twice: the `zellaude.json` read falls back to a built-in
  default, and `Settings` is `#[serde(default)]`.
- **Narrowing is intentional, and is the null rule's sibling:** a user who removes a name mid-session
  produces a smaller, non-null object, which the merge takes wholesale — so a richer capture is
  replaced by a poorer one without passing through null. That is correct and deliberate: the config is
  a redaction control, so a name the user just removed must not survive in the cache. Written down
  because a reader who has just absorbed "null never overwrites a capture" will otherwise read a
  shrinking `launch_env` as the bug it is not.
