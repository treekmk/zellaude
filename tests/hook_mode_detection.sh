#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TEST_DIR=$(mktemp -d)
CAPTURE_FILE="$TEST_DIR/payload.json"
trap 'rm -rf "$TEST_DIR"' EXIT

mkdir -p "$TEST_DIR/bin"
mkdir -p "$TEST_DIR/runtime"
# The hook reads the launch-env allowlist from the settings file under HOME, so
# without this the developer's own zellaude.json decides what these cases see.
mkdir -p "$TEST_DIR/home"
export HOME="$TEST_DIR/home"
cat > "$TEST_DIR/bin/zellij" <<'FAKE_ZELLIJ'
#!/usr/bin/env bash
printf '%s' "${!#}" > "$ZELLAUDE_TEST_CAPTURE"
FAKE_ZELLIJ
chmod +x "$TEST_DIR/bin/zellij"

cat > "$TEST_DIR/fake-claude" <<'FAKE_CLAUDE'
#!/usr/bin/env bash
printf '%s' "$ZELLAUDE_TEST_INPUT" | "$ZELLAUDE_TEST_HOOK"
FAKE_CLAUDE
chmod +x "$TEST_DIR/fake-claude"

run_hook() {
  local client=$1
  local input=$2
  local expected=$3
  local mode=${4:-}
  local expected_marker=${5:-skip}
  local expected_session_id=${6:-skip}
  local expected_subagent=${7:-skip}
  local actual actual_marker actual_session_id actual_subagent

  : > "$CAPTURE_FILE"
  if [ "$client" = "codex" ]; then
    printf '%s' "$input" |
      env -u CLAUDE_EFFORT \
        -u CLAUDE_CODE_EFFORT_LEVEL \
        -u ZELLAUDE_CLAUDE_MODE \
        PATH="$TEST_DIR/bin:$PATH" \
        XDG_RUNTIME_DIR="$TEST_DIR/runtime" \
        ZELLIJ_SESSION_NAME="test-session" \
        ZELLIJ_PANE_ID="7" \
        ZELLAUDE_TEST_CAPTURE="$CAPTURE_FILE" \
        "$PROJECT_DIR/scripts/zellaude-hook.sh" --client codex >/dev/null
  else
    printf '%s' "$input" |
      env -u CLAUDE_EFFORT \
        -u CLAUDE_CODE_EFFORT_LEVEL \
        ZELLAUDE_CLAUDE_MODE="$mode" \
        PATH="$TEST_DIR/bin:$PATH" \
        XDG_RUNTIME_DIR="$TEST_DIR/runtime" \
        ZELLIJ_SESSION_NAME="test-session" \
        ZELLIJ_PANE_ID="7" \
        ZELLAUDE_TEST_CAPTURE="$CAPTURE_FILE" \
        "$PROJECT_DIR/scripts/zellaude-hook.sh" >/dev/null
  fi

  actual=$(jq -c '.rainbow_name' "$CAPTURE_FILE")
  if [ "$actual" != "$expected" ]; then
    printf 'expected %s %s, got %s\n' "$client" "$expected" "$actual" >&2
    printf 'payload: %s\n' "$(cat "$CAPTURE_FILE")" >&2
    exit 1
  fi

  if [ "$expected" = "null" ]; then
    jq -e '.rainbow_mode_ts_ms == null' "$CAPTURE_FILE" >/dev/null
  else
    jq -e '
      (.rainbow_mode_ts_ms | type) == "number"
      and .rainbow_mode_ts_ms == .ts_ms
    ' "$CAPTURE_FILE" >/dev/null
  fi

  if [ "$expected_marker" != "skip" ]; then
    actual_marker=$(jq -c '.rainbow_mode_marker' "$CAPTURE_FILE")
    if [ "$actual_marker" != "$expected_marker" ]; then
      printf 'expected %s marker %s, got %s\n' \
        "$client" "$expected_marker" "$actual_marker" >&2
      exit 1
    fi
  fi

  if [ "$expected_session_id" != "skip" ]; then
    actual_session_id=$(jq -c '.session_id' "$CAPTURE_FILE")
    if [ "$actual_session_id" != "$expected_session_id" ]; then
      printf 'expected %s session id %s, got %s\n' \
        "$client" "$expected_session_id" "$actual_session_id" >&2
      exit 1
    fi
  fi

  if [ "$expected_subagent" != "skip" ]; then
    actual_subagent=$(jq -c '.is_subagent' "$CAPTURE_FILE")
    if [ "$actual_subagent" != "$expected_subagent" ]; then
      printf 'expected %s subagent %s, got %s\n' \
        "$client" "$expected_subagent" "$actual_subagent" >&2
      exit 1
    fi
  fi
}

run_launch_flag() {
  local effort=$1
  local input=$2
  local expected=$3
  local expected_marker=$4
  local actual actual_marker

  : > "$CAPTURE_FILE"
  env -u CLAUDE_EFFORT \
    -u CLAUDE_CODE_EFFORT_LEVEL \
    -u ZELLAUDE_CLAUDE_MODE \
    PATH="$TEST_DIR/bin:$PATH" \
    XDG_RUNTIME_DIR="$TEST_DIR/runtime" \
    ZELLIJ_SESSION_NAME="test-session" \
    ZELLIJ_PANE_ID="7" \
    ZELLAUDE_TEST_CAPTURE="$CAPTURE_FILE" \
    ZELLAUDE_TEST_HOOK="$PROJECT_DIR/scripts/zellaude-hook.sh" \
    ZELLAUDE_TEST_INPUT="$input" \
    "$TEST_DIR/fake-claude" --effort "$effort" >/dev/null

  actual=$(jq -c '.rainbow_name' "$CAPTURE_FILE")
  actual_marker=$(jq -c '.rainbow_mode_marker' "$CAPTURE_FILE")
  if [ "$actual" != "$expected" ] ||
     [ "$actual_marker" != "$expected_marker" ]; then
    printf 'launch --effort %s expected %s/%s, got %s/%s\n' \
      "$effort" "$expected" "$expected_marker" "$actual" "$actual_marker" >&2
    exit 1
  fi
}

run_inline_settings_flag() {
  local settings=$1
  local expected=$2
  local actual

  : > "$CAPTURE_FILE"
  env -u CLAUDE_EFFORT \
    -u CLAUDE_CODE_EFFORT_LEVEL \
    -u ZELLAUDE_CLAUDE_MODE \
    PATH="$TEST_DIR/bin:$PATH" \
    XDG_RUNTIME_DIR="$TEST_DIR/runtime" \
    ZELLIJ_SESSION_NAME="test-session" \
    ZELLIJ_PANE_ID="7" \
    ZELLAUDE_TEST_CAPTURE="$CAPTURE_FILE" \
    ZELLAUDE_TEST_HOOK="$PROJECT_DIR/scripts/zellaude-hook.sh" \
    ZELLAUDE_TEST_INPUT='{"session_id":"claude-settings","hook_event_name":"SessionStart"}' \
    "$TEST_DIR/fake-claude" --settings "$settings" >/dev/null

  actual=$(jq -c '.rainbow_name' "$CAPTURE_FILE")
  if [ "$actual" != "$expected" ]; then
    printf 'inline settings %s expected %s, got %s\n' \
      "$settings" "$expected" "$actual" >&2
    exit 1
  fi
}

CODEX_TRANSCRIPT="$TEST_DIR/codex.jsonl"
cat > "$CODEX_TRANSCRIPT" <<'CODEX_JSONL'
{"type":"turn_context","payload":{"turn_id":"turn-ultra","effort":"ultra"}}
{"type":"turn_context","payload":{"turn_id":"turn-high","effort":"high"}}
{"type":"partially-written"
CODEX_JSONL

run_hook codex "$(jq -nc \
  --arg transcript "$CODEX_TRANSCRIPT" \
  '{session_id:"codex-ultra",hook_event_name:"PreToolUse",turn_id:"turn-ultra",transcript_path:$transcript}')" \
  true
STATE_FILE="$TEST_DIR/runtime/zellaude-$(id -u)/test-session.7.json"
jq -e '
  .session_id == "codex-ultra"
  and .rainbow_name == true
' "$STATE_FILE" >/dev/null
INITIAL_MODE_TS=$(jq -r '.rainbow_mode_ts_ms' "$STATE_FILE")

# An unknown event in the same root session must preserve the cached mode.
run_hook codex \
  '{"session_id":"codex-ultra","hook_event_name":"Notification"}' \
  null
jq -e --argjson mode_ts "$INITIAL_MODE_TS" '
  .session_id == "codex-ultra"
  and .rainbow_name == true
  and .rainbow_mode_ts_ms == $mode_ts
  and .ts_ms >= $mode_ts
' "$STATE_FILE" >/dev/null
XDG_RUNTIME_DIR="$TEST_DIR/runtime" \
  "$PROJECT_DIR/scripts/zellaude-hook.sh" --restore test-session |
  jq -e '
    .session_id == "codex-ultra"
    and .pane_id == 7
    and .rainbow_name == true
  ' >/dev/null

# Cache writes can complete out of order when asynchronous hooks overlap. An
# older hook must not replace the newer state merely because its process exits
# last.
CACHE_FUTURE_TS=9999999999999
jq --arg session_id "cache-order" \
  --argjson ts_ms "$CACHE_FUTURE_TS" '
    .session_id = $session_id
    | .hook_event = "PreToolUse"
    | .ts_ms = $ts_ms
    | .rainbow_name = true
    | .rainbow_mode_marker = "newer-mode"
  ' "$STATE_FILE" > "$STATE_FILE.tmp"
mv "$STATE_FILE.tmp" "$STATE_FILE"
run_hook codex \
  '{"session_id":"cache-order","hook_event_name":"PreToolUse","reasoning_effort":"high"}' \
  false
jq -e --argjson ts_ms "$CACHE_FUTURE_TS" '
  .session_id == "cache-order"
  and .ts_ms == $ts_ms
  and .rainbow_name == true
  and .rainbow_mode_marker == "newer-mode"
' "$STATE_FILE" >/dev/null

# A delayed SessionEnd from the previous process in a reused pane must not
# erase the cache owned by its replacement.
rm -f "$STATE_FILE"
run_hook codex \
  '{"session_id":"cache-new-owner","hook_event_name":"PreToolUse","reasoning_effort":"ultra"}' \
  true
run_hook codex \
  '{"session_id":"cache-old-owner","hook_event_name":"SessionEnd"}' \
  null
jq -e '
  .session_id == "cache-new-owner"
  and .rainbow_name == true
' "$STATE_FILE" >/dev/null

# The matching owner's normal end still removes its own cache entry.
run_hook codex \
  '{"session_id":"cache-new-owner","hook_event_name":"SessionEnd"}' \
  null
[ ! -e "$STATE_FILE" ]

run_hook codex "$(jq -nc \
  --arg transcript "$CODEX_TRANSCRIPT" \
  '{session_id:"codex-high",hook_event_name:"PreToolUse",turn_id:"turn-high",transcript_path:$transcript}')" \
  false
run_hook codex "$(jq -nc \
  --arg transcript "$CODEX_TRANSCRIPT" \
  '{session_id:"codex-missing-turn",hook_event_name:"PreToolUse",turn_id:"missing-turn",transcript_path:$transcript}')" \
  null

CODEX_LONG_TRANSCRIPT="$TEST_DIR/codex-long.jsonl"
cat > "$CODEX_LONG_TRANSCRIPT" <<'CODEX_LONG_JSONL'
{"type":"turn_context","payload":{"turn_id":"turn-long-ultra","effort":"ultra"}}
CODEX_LONG_JSONL
{
  printf '{"type":"response_item","payload":"'
  head -c 2200000 /dev/zero | tr '\0' x
  printf '"}\n'
} >> "$CODEX_LONG_TRANSCRIPT"
run_hook codex "$(jq -nc \
  --arg transcript "$CODEX_LONG_TRANSCRIPT" \
  '{session_id:"codex-long",hook_event_name:"PreToolUse",turn_id:"turn-long-ultra",transcript_path:$transcript}')" \
  true
run_hook codex "$(jq -nc \
  --arg transcript "$CODEX_LONG_TRANSCRIPT" \
  '{session_id:"codex-long-resume",hook_event_name:"SessionStart",transcript_path:$transcript}')" \
  true

CODEX_AGENT_TRANSCRIPT="$TEST_DIR/codex-agent.jsonl"
cat > "$CODEX_AGENT_TRANSCRIPT" <<'CODEX_AGENT_JSONL'
{"timestamp":"2026-07-30T00:00:00Z","type":"turn_context","payload":{"turn_id":"child-turn","model":"gpt-test","effort":"ultra"}}
CODEX_AGENT_JSONL
run_hook codex "$(jq -nc \
  --arg transcript "$CODEX_TRANSCRIPT" \
  --arg agent_transcript "$CODEX_AGENT_TRANSCRIPT" \
  '{session_id:"codex-agent",hook_event_name:"SubagentStop",turn_id:"child-turn",transcript_path:$transcript,agent_transcript_path:$agent_transcript}')" \
  null \
  "" \
  skip \
  '""' \
  true

run_hook codex \
  '{"session_id":"codex-unknown","hook_event_name":"SessionStart"}' \
  null
run_hook codex \
  '{"session_id":"codex-child","hook_event_name":"PreToolUse","agent_id":"child-1","reasoning_effort":"high"}' \
  null \
  "" \
  skip \
  '""' \
  true

STATE_BEFORE_CHILD=$(cat "$STATE_FILE")
run_hook codex \
  '{"session_id":"codex-child","hook_event_name":"PostToolUse","agent_id":"child-1","reasoning_effort":"high"}' \
  null \
  "" \
  skip \
  '""' \
  true
[ "$(cat "$STATE_FILE")" = "$STATE_BEFORE_CHILD" ]

CODEX_INTERNAL_CHILD_TRANSCRIPT="$TEST_DIR/codex-internal-child.jsonl"
cat > "$CODEX_INTERNAL_CHILD_TRANSCRIPT" <<'CODEX_INTERNAL_CHILD_JSONL'
{"type":"session_meta","payload":{"source":{"subagent":{"review":{}}}}}
{"type":"turn_context","payload":{"turn_id":"internal-child-turn","effort":"high"}}
CODEX_INTERNAL_CHILD_JSONL
run_hook codex "$(jq -nc \
  --arg transcript "$CODEX_INTERNAL_CHILD_TRANSCRIPT" \
  '{session_id:"codex-internal-child",hook_event_name:"PreToolUse",turn_id:"internal-child-turn",transcript_path:$transcript}')" \
  null \
  "" \
  skip \
  '""' \
  true

CLAUDE_ULTRA_TRANSCRIPT="$TEST_DIR/claude-ultra.jsonl"
cat > "$CLAUDE_ULTRA_TRANSCRIPT" <<'CLAUDE_ULTRA_JSONL'
{"type":"user","uuid":"effort-command","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"effort-command","message":{"content":"<local-command-stdout>Set effort level to ultracode (this session only): xhigh + dynamic workflow orchestration</local-command-stdout>"}}
CLAUDE_ULTRA_JSONL

run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_ULTRA_TRANSCRIPT" \
  '{session_id:"claude-ultra",hook_event_name:"UserPromptSubmit",transcript_path:$transcript,effort:{level:"xhigh"}}')" \
  true

CLAUDE_LONG_TRANSCRIPT="$TEST_DIR/claude-long.jsonl"
cat > "$CLAUDE_LONG_TRANSCRIPT" <<'CLAUDE_LONG_JSONL'
{"type":"user","uuid":"long-effort-command","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"long-effort-command","message":{"content":"<local-command-stdout>Set effort level to ultracode (this session only)</local-command-stdout>"}}
CLAUDE_LONG_JSONL
{
  printf '{"type":"assistant","message":{"content":"'
  head -c 2200000 /dev/zero | tr '\0' x
  printf '"}}\n'
} >> "$CLAUDE_LONG_TRANSCRIPT"
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_LONG_TRANSCRIPT" \
  '{session_id:"claude-long",hook_event_name:"SessionRestore",transcript_path:$transcript,effort:{level:"xhigh"}}')" \
  true

CLAUDE_XHIGH_TRANSCRIPT="$TEST_DIR/claude-xhigh.jsonl"
cat > "$CLAUDE_XHIGH_TRANSCRIPT" <<'CLAUDE_XHIGH_JSONL'
{"type":"user","uuid":"effort-command","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"effort-command","message":{"content":"<local-command-stdout>Set effort level to xhigh (saved as your default)</local-command-stdout>"}}
CLAUDE_XHIGH_JSONL

run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_XHIGH_TRANSCRIPT" \
  '{session_id:"claude-xhigh",hook_event_name:"UserPromptSubmit",transcript_path:$transcript,effort:{level:"xhigh"}}')" \
  false

# Attach recovery treats the target process launch mode as a baseline. A
# successful /effort command at or after the registry start time supersedes it.
CLAUDE_AFTER_XHIGH_TRANSCRIPT="$TEST_DIR/claude-after-xhigh.jsonl"
cat > "$CLAUDE_AFTER_XHIGH_TRANSCRIPT" <<'CLAUDE_AFTER_XHIGH_JSONL'
{"type":"user","uuid":"after-xhigh","timestamp":"2026-07-30T10:00:00.100Z","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"after-xhigh","timestamp":"2026-07-30T10:00:00.123Z","message":{"content":"<local-command-stdout>Set effort level to xhigh (this session only)</local-command-stdout>"}}
CLAUDE_AFTER_XHIGH_JSONL
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_AFTER_XHIGH_TRANSCRIPT" \
  '{session_id:"claude-restore-after-xhigh",hook_event_name:"SessionRestore",transcript_path:$transcript,session_started_at_ms:1785405600000,launch_ultracode:true}')" \
  false \
  "" \
  '"after-xhigh"'

CLAUDE_AT_START_ULTRA_TRANSCRIPT="$TEST_DIR/claude-at-start-ultra.jsonl"
cat > "$CLAUDE_AT_START_ULTRA_TRANSCRIPT" <<'CLAUDE_AT_START_ULTRA_JSONL'
{"type":"user","uuid":"at-start-ultra","timestamp":"2026-07-30T10:00:00.000Z","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"at-start-ultra","timestamp":"2026-07-30T10:00:00.000Z","message":{"content":"<local-command-stdout>Set effort level to ultracode (this session only)</local-command-stdout>"}}
CLAUDE_AT_START_ULTRA_JSONL
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_AT_START_ULTRA_TRANSCRIPT" \
  '{session_id:"claude-restore-at-start",hook_event_name:"SessionRestore",transcript_path:$transcript,session_started_at_ms:1785405600000,launch_ultracode:false}')" \
  true \
  "" \
  '"at-start-ultra"'

# Historical commands in a resumed transcript cannot override an explicit
# choice made by the newer process invocation.
CLAUDE_BEFORE_XHIGH_TRANSCRIPT="$TEST_DIR/claude-before-xhigh.jsonl"
cat > "$CLAUDE_BEFORE_XHIGH_TRANSCRIPT" <<'CLAUDE_BEFORE_XHIGH_JSONL'
{"type":"user","uuid":"before-xhigh","timestamp":"2026-07-30T09:59:59.900Z","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"before-xhigh","timestamp":"2026-07-30T09:59:59.999Z","message":{"content":"<local-command-stdout>Set effort level to xhigh (this session only)</local-command-stdout>"}}
CLAUDE_BEFORE_XHIGH_JSONL
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_BEFORE_XHIGH_TRANSCRIPT" \
  '{session_id:"claude-restore-before-xhigh",hook_event_name:"SessionRestore",transcript_path:$transcript,session_started_at_ms:1785405600000,launch_ultracode:true}')" \
  true \
  "" \
  '"before-xhigh"'

run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_ULTRA_TRANSCRIPT" \
  '{session_id:"claude-restore-undated",hook_event_name:"SessionRestore",transcript_path:$transcript,session_started_at_ms:1785405600000,launch_ultracode:false}')" \
  false \
  "" \
  '"effort-command"'

run_hook claude \
  '{"session_id":"claude-ambiguous","hook_event_name":"PreToolUse","effort":{"level":"xhigh"}}' \
  null
run_hook claude \
  '{"session_id":"claude-high","hook_event_name":"PreToolUse","effort":{"level":"high"}}' \
  false
run_hook claude \
  '{"session_id":"claude-explicit","hook_event_name":"SessionStart","ultracode":true}' \
  true
run_hook claude \
  '{"session_id":"claude-child","hook_event_name":"PreToolUse","agent_id":"child-1","ultracode":false}' \
  null \
  "" \
  skip \
  '""' \
  true
run_hook claude \
  '{"session_id":"claude-sentinel","hook_event_name":"SessionStart"}' \
  true \
  ultracode

CLAUDE_FAILED_TRANSCRIPT="$TEST_DIR/claude-failed.jsonl"
cat > "$CLAUDE_FAILED_TRANSCRIPT" <<'CLAUDE_FAILED_JSONL'
{"type":"user","uuid":"failed-command","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"failed-command","message":{"content":"<local-command-stdout>Failed to set effort level: ultracode is unavailable</local-command-stdout>"}}
CLAUDE_FAILED_JSONL
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_FAILED_TRANSCRIPT" \
  '{session_id:"claude-failed",hook_event_name:"UserPromptSubmit",transcript_path:$transcript,effort:{level:"xhigh"}}')" \
  null

CLAUDE_INCOMPLETE_TRANSCRIPT="$TEST_DIR/claude-incomplete.jsonl"
cat > "$CLAUDE_INCOMPLETE_TRANSCRIPT" <<'CLAUDE_INCOMPLETE_JSONL'
{"type":"user","uuid":"old-command","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"old-command","message":{"content":"<local-command-stdout>Set effort level to ultracode (this session only)</local-command-stdout>"}}
{"type":"user","uuid":"new-command","message":{"content":"<command-name>/effort</command-name>"}}
CLAUDE_INCOMPLETE_JSONL
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_INCOMPLETE_TRANSCRIPT" \
  '{session_id:"claude-incomplete",hook_event_name:"UserPromptSubmit",transcript_path:$transcript,effort:{level:"xhigh"}}')" \
  null

CLAUDE_TOGGLES_TRANSCRIPT="$TEST_DIR/claude-toggles.jsonl"
cat > "$CLAUDE_TOGGLES_TRANSCRIPT" <<'CLAUDE_TOGGLES_JSONL'
{"type":"user","uuid":"ultra-command","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"ultra-command","message":{"content":"<local-command-stdout>Set effort level to ultracode (this session only)</local-command-stdout>"}}
{"type":"user","uuid":"xhigh-command","message":{"content":"<command-name>/effort</command-name>"}}
{"type":"user","parentUuid":"xhigh-command","message":{"content":"<local-command-stdout>Set effort level to xhigh (saved as your default)</local-command-stdout>"}}
CLAUDE_TOGGLES_JSONL
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_TOGGLES_TRANSCRIPT" \
  '{session_id:"claude-toggles",hook_event_name:"UserPromptSubmit",transcript_path:$transcript,effort:{level:"xhigh"}}')" \
  false \
  "" \
  '"xhigh-command"'

# A fresh launch choice must outrank historical commands in a resumed file.
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_XHIGH_TRANSCRIPT" \
  '{session_id:"claude-resume-ultra",hook_event_name:"SessionStart",transcript_path:$transcript}')" \
  true \
  ultracode \
  '"effort-command"'
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_ULTRA_TRANSCRIPT" \
  '{session_id:"claude-resume-high",hook_event_name:"SessionStart",transcript_path:$transcript,effort:{level:"high"}}')" \
  false
run_launch_flag ultracode "$(jq -nc \
  --arg transcript "$CLAUDE_XHIGH_TRANSCRIPT" \
  '{session_id:"claude-launch-ultra",hook_event_name:"SessionStart",transcript_path:$transcript}')" \
  true \
  '"effort-command"'
run_launch_flag high "$(jq -nc \
  --arg transcript "$CLAUDE_ULTRA_TRANSCRIPT" \
  '{session_id:"claude-launch-high",hook_event_name:"SessionStart",transcript_path:$transcript}')" \
  false \
  '"effort-command"'

# A launch override carries the latest transcript marker as its baseline.
# Replaying that exact marker on a later hook must not replace the cached mode
# or its observation timestamp; Rust applies the same marker-idempotence rule.
LAUNCH_OVERRIDE_MODE_TS=$(jq -r '.rainbow_mode_ts_ms' "$STATE_FILE")
run_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_ULTRA_TRANSCRIPT" \
  '{session_id:"claude-launch-high",hook_event_name:"UserPromptSubmit",transcript_path:$transcript,effort:{level:"xhigh"}}')" \
  true \
  "" \
  '"effort-command"'
jq -e --argjson mode_ts "$LAUNCH_OVERRIDE_MODE_TS" '
  .session_id == "claude-launch-high"
  and .rainbow_name == false
  and .rainbow_mode_marker == "effort-command"
  and .rainbow_mode_ts_ms == $mode_ts
  and .ts_ms >= $mode_ts
' "$STATE_FILE" >/dev/null

run_inline_settings_flag '{"ultracode":true}' true
run_inline_settings_flag '{"ultracode":false,"other":true}' false

# --- launch environment capture ---------------------------------------------

# find_agent_pid matches on comm, and a shebang script reports comm=bash, so the
# stand-in agent must be a copy of the bash binary named for the client. Kept
# off PATH: these names would otherwise shadow a real client for other cases.
LAUNCH_ENV_BIN="$TEST_DIR/launch-bin"
LAUNCH_ENV_PROC_ROOT="$TEST_DIR/launch-proc"
LAUNCH_ENV_ENVIRON="$TEST_DIR/launch-environ"
LAUNCH_ENV_STDOUT="$TEST_DIR/launch-stdout.json"
mkdir -p "$LAUNCH_ENV_BIN" "$LAUNCH_ENV_PROC_ROOT"
cp "$(command -v bash)" "$LAUNCH_ENV_BIN/claude"
cp "$(command -v bash)" "$LAUNCH_ENV_BIN/codex"

# The agent publishes its own environ, because only it knows the pid that
# find_agent_pid will resolve to. An empty fixture publishes nothing, which is
# how the not-captured path is reached.
cat > "$TEST_DIR/launch-agent.sh" <<'LAUNCH_AGENT'
if [ -s "$ZELLAUDE_TEST_ENVIRON" ]; then
  mkdir -p "$ZELLAUDE_PROC_ROOT/$$"
  cp "$ZELLAUDE_TEST_ENVIRON" "$ZELLAUDE_PROC_ROOT/$$/environ"
fi
printf '%s' "$ZELLAUDE_TEST_INPUT" |
  "$ZELLAUDE_TEST_HOOK" ${ZELLAUDE_TEST_HOOK_ARGS:-}
LAUNCH_AGENT

write_environ() {
  local entry
  : > "$LAUNCH_ENV_ENVIRON"
  for entry in "$@"; do
    printf '%s\0' "$entry" >> "$LAUNCH_ENV_ENVIRON"
  done
}

run_launch_env_hook() {
  local client=$1 input=$2 hook_args=""
  shift 2
  [ "$client" != "codex" ] || hook_args="--client codex"

  : > "$CAPTURE_FILE"
  env -u CLAUDE_EFFORT \
    -u CLAUDE_CODE_EFFORT_LEVEL \
    -u ZELLAUDE_CLAUDE_MODE \
    PATH="$TEST_DIR/bin:$PATH" \
    XDG_RUNTIME_DIR="$TEST_DIR/runtime" \
    ZELLIJ_SESSION_NAME="test-session" \
    ZELLIJ_PANE_ID="7" \
    ZELLAUDE_TEST_CAPTURE="$CAPTURE_FILE" \
    ZELLAUDE_TEST_HOOK="$PROJECT_DIR/scripts/zellaude-hook.sh" \
    ZELLAUDE_TEST_HOOK_ARGS="$hook_args" \
    ZELLAUDE_TEST_INPUT="$input" \
    ZELLAUDE_TEST_ENVIRON="$LAUNCH_ENV_ENVIRON" \
    ZELLAUDE_PROC_ROOT="$LAUNCH_ENV_PROC_ROOT" \
    "$@" \
    "$LAUNCH_ENV_BIN/$client" "$TEST_DIR/launch-agent.sh" > "$LAUNCH_ENV_STDOUT"
}

rm -f "$STATE_FILE"
write_environ \
  'ANTHROPIC_BASE_URL=http://127.0.0.1:9/' \
  'ANTHROPIC_AUTH_TOKEN=local' \
  'ANTHROPIC_API_KEY=not-a-real-key' \
  'CLAUDE_CODE_EFFORT_LEVEL=high' \
  'ANTHROPIC_BASE_URL_EXTRA=prefix-trap' \
  'CLAUDE_CODE_BRIDGE_SESSION_ID=injected' \
  'PATH=/usr/bin'
run_launch_env_hook claude \
  '{"session_id":"launch-env","hook_event_name":"SessionStart"}'
# Config names verbatim, secrets by tier, and the whole key set: an exact-name
# allowlist cannot leak by construction, so what is worth asserting is that the
# reader captured nothing beyond its list.
jq -e '
  .launch_env.ANTHROPIC_BASE_URL == "http://127.0.0.1:9/"
  and .launch_env.ANTHROPIC_AUTH_TOKEN == "local"
  and .launch_env.ANTHROPIC_API_KEY == "<set>"
  and .launch_env.CLAUDE_CODE_EFFORT_LEVEL == "high"
  and (.launch_env | keys) == [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "CLAUDE_CODE_EFFORT_LEVEL"
  ]
' "$CAPTURE_FILE" >/dev/null
if grep -q 'not-a-real-key' "$CAPTURE_FILE" "$STATE_FILE"; then
  printf 'a secret value reached the payload or the cache\n' >&2
  exit 1
fi

# A launch environ is fixed at exec, so within one session a null is a failed
# read, never a newer answer — it must not erase a capture, on any event.
# Anything non-null still lands, or the guard would be a blanket keep-previous.
write_environ
run_launch_env_hook claude \
  '{"session_id":"launch-env","hook_event_name":"PostToolUse"}' \
  CLAUDE_EFFORT=max
jq -e '.launch_env == null and .current_effort_level == "max"' \
  "$CAPTURE_FILE" >/dev/null
jq -e '
  .launch_env.ANTHROPIC_BASE_URL == "http://127.0.0.1:9/"
  and .current_effort_level == "max"
' "$STATE_FILE" >/dev/null

# The two merge rules coexist rather than one swallowing the other: a
# SessionStart is the authoritative new signal for rainbow_name, and in the same
# event the launch fields still keep what a failed read could not supply.
run_launch_env_hook claude \
  '{"session_id":"launch-env","hook_event_name":"SessionStart"}'
jq -e '
  .rainbow_name == null
  and .launch_env.ANTHROPIC_BASE_URL == "http://127.0.0.1:9/"
  and .current_effort_level == "max"
' "$STATE_FILE" >/dev/null

XDG_RUNTIME_DIR="$TEST_DIR/runtime" \
  "$PROJECT_DIR/scripts/zellaude-hook.sh" --restore test-session |
  jq -e '
    .session_id == "launch-env"
    and (has("launch_env") | not)
    and .current_effort_level == "max"
  ' >/dev/null

# A different session inherits nothing: the previous agent's environ is not this
# agent's, however recently it was cached.
run_launch_env_hook claude \
  '{"session_id":"launch-env-other","hook_event_name":"SessionStart"}'
jq -e '.launch_env == null and .current_effort_level == null' \
  "$STATE_FILE" >/dev/null

# Read but nothing matched is not the same as never read.
write_environ 'PATH=/usr/bin'
run_launch_env_hook claude \
  '{"session_id":"launch-env-empty","hook_event_name":"SessionStart"}'
jq -e '.launch_env == {}' "$CAPTURE_FILE" >/dev/null

# --inspect runs outside the agent's process tree, so the environment reachable
# there is the caller's, not the agent's — null even when a readable environ and
# a resolvable agent_pid would otherwise produce a capture.
write_environ 'ANTHROPIC_BASE_URL=http://127.0.0.1:9/'
run_launch_env_hook claude \
  '{"session_id":"launch-env-inspect","hook_event_name":"PostToolUse"}' \
  ZELLAUDE_TEST_HOOK_ARGS=--inspect
jq -e '.launch_env == null and (.agent_pid | type) == "number"' \
  "$LAUNCH_ENV_STDOUT" >/dev/null

# Every effort source is normalized, so an uppercase CLAUDE_EFFORT reaches the
# payload lowercased and takes the explicit-effort early return instead of
# falling through to the transcript's ultracode state.
write_environ
run_launch_env_hook claude "$(jq -nc \
  --arg transcript "$CLAUDE_ULTRA_TRANSCRIPT" \
  '{session_id:"launch-env-effort",hook_event_name:"PreToolUse",transcript_path:$transcript}')" \
  CLAUDE_EFFORT=HIGH
jq -e '.current_effort_level == "high" and .rainbow_name == false' \
  "$CAPTURE_FILE" >/dev/null
run_launch_env_hook claude \
  '{"session_id":"launch-env-effort","hook_event_name":"PostToolUse","effort":{"level":"MAX"}}' \
  CLAUDE_EFFORT=high
jq -e '.current_effort_level == "max"' "$CAPTURE_FILE" >/dev/null

# The asymmetry on a codex pane is deliberate, and the two halves are asserted
# together so it cannot be tidied away: an inherited ANTHROPIC_BASE_URL was
# genuinely in this process's environ at exec, while an inherited CLAUDE_EFFORT
# would assert a Claude session's effort about a codex agent.
write_environ \
  'ANTHROPIC_BASE_URL=http://127.0.0.1:9/' \
  'CODEX_HOME=/tmp/codex-home' \
  'CODEX_API_KEY=not-a-real-key'
run_launch_env_hook codex \
  '{"session_id":"launch-env-codex","hook_event_name":"PreToolUse"}' \
  CLAUDE_EFFORT=high
jq -e '
  .client == "codex"
  and .current_effort_level == null
  and .launch_env.ANTHROPIC_BASE_URL == "http://127.0.0.1:9/"
  and .launch_env.CODEX_HOME == "/tmp/codex-home"
  and .launch_env.CODEX_API_KEY == "<set>"
' "$CAPTURE_FILE" >/dev/null

printf 'hook mode detection tests passed\n'
