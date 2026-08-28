## Plan: Capture Claude Launch Environment

Record each agent's launch environment in the per-pane state cache so relaunch tools (`zellmv`) can
reproduce it instead of silently reaching the default Anthropic endpoint. The hook gains two payload
fields: `launch_env`, read from the agent's own `/proc/<agent_pid>/environ` against one client-agnostic
allowlist of exact variable names, and `current_effort_level`, resolved through the precedence the hook
already uses. Secret-named variables record a marker, not their value, with one literal escape.

**Context**
Env-prefixed launches (`ANTHROPIC_BASE_URL=… ANTHROPIC_AUTH_TOKEN=… claude --effort xhigh`, an abbr for
per-session backends) are invisible to the cache: it proves identity (`session_id`, `cwd`, `agent_pid`)
but records nothing about how the agent started, so any relaunch reaches the default endpoint. Capture
alone changes nothing user-visible — the value arrives with the replay half,
`feature/zellmv-faithful-resume` in my-terminal-setup. This is the producer side of that contract.
Branch `feature/capture-claude-launch-env`; the merge task merges in `develop`.
Mode: ask

**Approach**
- Phase 1 (T1–T2): add the allowlist and the environ reader to `scripts/zellaude-hook.sh`; extract the
  effort-level precedence now buried inside `detect_claude_rainbow`.
- Phase 2 (T3): wire both fields into `PAYLOAD`, guard them in the `persist_root_state` merge, strip
  `launch_env` from the `--restore` output.
- Phase 3 (T4): fixture-driven tests over both capture paths, the secret policy, and the merge guard.
- T3 needs T1 and T2; T4 needs T3. T1–T3 edit one file and are strictly sequential.

**Relevant files**
- `scripts/zellaude-hook.sh` — the whole change: allowlist + reader near `find_agent_pid`, the extracted
  effort resolver, the `PAYLOAD` jq block, the merge jq in `persist_root_state`, the projection in
  `restore_cached_states`.
- `tests/hook_mode_detection.sh` — the harness already runs the real hook with a fake `zellij` capturing
  `PAYLOAD`; new payload-field cases belong beside it.
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
CLAUDE_CODE_USE_VERTEX CLAUDE_CODE_EFFORT_LEVEL ZELLAUDE_CLAUDE_MODE CODEX_HOME"  # verbatim
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
resolve_effort_level() { :; }
```

New payload fields:

```
launch_env            object | null   # null = not captured; {} = captured, nothing matched
current_effort_level  string | null   # effort as of this event's ts_ms, NOT launch-time
```

- `current_effort_level` departs from the seed's `effort_level` deliberately. Beside `launch_env` a bare
  `effort_level` reads as launch state, and the contract already carries `launch_effort_level`
  (`zellaude-attach.sh:376`) and `launch_env.CLAUDE_CODE_EFFORT_LEVEL`, both launch-time. The qualifier
  separates the three. Free to rename — the consumer does not exist yet.

**Verification**
- `bash -n scripts/zellaude-hook.sh`; `shellcheck` on it if available (absent from CI, so not a gate).
- `tests/hook_mode_detection.sh` and `tests/attach_detection.sh` must stay green — they cover every
  existing payload field and the `--restore` path being narrowed.
- `cargo build --release --target wasm32-wasip1`. `cargo test` cannot run here (wasm default target, no
  wasmtime; the host target needs absent OpenSSL), so the shell suites are the executed checks.
- Cheap guard for the Rust coupling, since `cargo test` cannot run here:
  `grep -q '^state_cache_key() {' scripts/zellaude-hook.sh` — `src/manifest.rs` slices that function out
  of the embedded script by literal search, and no executable test covers it on this machine.
- **E2E** (required): a real `claude`, a real Zellij session and the **branch's** hook write a real
  cache file — in a staged home, so nothing the user is running is touched.
  1. `export ZELLAUDE_E2E=/tmp/capture-claude-launch-env` and make `$ZELLAUDE_E2E/{home,runtime}` mode
     `700`. `runtime` must be `700` and user-owned or `state_cache_dir` will reject it.
  2. `ZELLAUDE_INSTALL_HOME=$ZELLAUDE_E2E/home ./install.sh --no-permissions` — the repo's own isolation
     override (README: "running an isolated test"). It installs **this branch's** hook and registers it
     in the staged Claude settings. The live `~/.config/zellij/plugins/zellaude-hook.sh` every agent on
     this box uses is never written.
  3. Start an isolated Zellij session named `zellaude-e2e-launchenv` (preflight: it must not already
     exist) and run one pane under a **controlled** environment — the environment is what this feature
     measures, so it cannot be left ambient. Unset every allowlisted name, then set only the four the
     case needs, the idiom `tests/hook_mode_detection.sh:36-38` already uses:
     `env -u ANTHROPIC_MODEL -u CLAUDE_CODE_USE_BEDROCK -u CLAUDE_CODE_USE_VERTEX -u ZELLAUDE_CLAUDE_MODE`
     `-u CODEX_HOME -u CODEX_API_KEY -u OPENAI_API_KEY -u CLAUDE_EFFORT`
     `HOME=$ZELLAUDE_E2E/home XDG_RUNTIME_DIR=$ZELLAUDE_E2E/runtime`
     `ANTHROPIC_BASE_URL=http://127.0.0.1:9/ ANTHROPIC_AUTH_TOKEN=local`
     `ANTHROPIC_API_KEY=e2e-not-a-real-key CLAUDE_CODE_EFFORT_LEVEL=high claude --effort high -p hi`.
     Not `env -i`: that would also wipe `ZELLIJ_SESSION_NAME` and `ZELLIJ_PANE_ID`, which the hook
     requires and exits on (`:263-264`). `HOME` must be the staged one — the installer registers the hook as the
     literal `${HOME}/.config/zellij/plugins/zellaude-hook.sh` (`install-hooks.sh:15`), so this is what
     makes Claude Code invoke the branch's script rather than the live one. The unroutable base URL is
     deliberate: `SessionStart` fires, the request dies immediately, **no API quota is spent**.
  4. Read `$ZELLAUDE_E2E/runtime/zellaude-$(id -u)/zellaude-e2e-launchenv.<pane>.json`. Staging
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
    `(.launch_env | keys) == ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL",`
    `"CLAUDE_CODE_EFFORT_LEVEL"]` — the whole key set, not a spot check. An exact-name allowlist cannot
    leak an injected variable by construction, so asserting one is absent proves nothing; asserting the
    set catches the failure that *is* possible, a reader capturing more than it should. This strictness
    is only sound because step 3 controls the environment.
    Finally `grep -q 'e2e-not-a-real-key' <cache file>` must **fail** — direct evidence that a value
    which reached `/proc` did not reach disk, which is the claim a reader of this feature most wants.
  - `.current_effort_level == "high"` is asserted **separately**, and a mismatch is reported, never
    fixed. Its only source on `SessionStart` is the inherited `CLAUDE_EFFORT`, and that a session whose
    effort was set on the command line exports it is an assumption of this PLAN, not an established
    fact — observed once, on a session already running at `xhigh`. Because step 3 unsets
    `CLAUDE_EFFORT`, a `"high"` here can only have been injected by this run's own agent, which makes
    this a real probe of the export semantics rather than a hope. A `null` is therefore a finding about
    `CLAUDE_EFFORT` and a correction to this PLAN, never a defect to patch around. The shell suites,
    where both sources are controllable by construction, are what gate the field's logic; only this can
    say what the vendor binary actually exports.
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
  only `rainbow_*`, so a `null` in either new field erases a good one. Both follow the rule
  `rainbow_name` already uses — on a matching `session_id`, a null `$current` keeps `$previous`. It
  matters most for `launch_env`: a launch environ is fixed at exec, so a later null is always
  stale-wrong rather than a legitimate update, and `null` means "not captured", which must not become a
  lie about a pane captured moments earlier. Contradicts the seed's claim that the merge already keeps
  the fields; it does not.
- **Effort precedence resolved once:** `.effort.level` from stdin, then `CLAUDE_EFFORT` — the precedence
  already at `zellaude-hook.sh:687-688`, extracted into `resolve_effort_level` and shared. Today's copy
  sits behind early returns inside `detect_claude_rainbow` and is out of scope when `PAYLOAD` is built;
  resolving it again would let one process answer the same question two ways. This makes the change
  wider than the seed's footprint implied. The env fallback is load-bearing: `.effort` does not ride
  every event type, so a stdin-only value would arrive `null` on `SessionStart` — destructive per above.
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
  read recomputes an identical result; one file read plus one `jq` is accepted on the hot path rather
  than adding a cache-validity scheme.
- **Not made configurable:** the allowlist is hardcoded, not read from `zellaude.json`. One visible list
  was the request; a settings surface was not, and would need its own validation and precedence rules.
