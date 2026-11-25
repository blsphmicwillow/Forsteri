#!/usr/bin/env bash
# Forsteri installer: crash-safe progress state.
#
# The live medium keeps one small file. It is written after each destructive
# phase, so a kill -9 between partitioning and base installation can resume at
# the right phase instead of wiping the disk again. It is intentionally plain
# text and never sourced as shell code.

FORSTERI_STATE_FILE=${FORSTERI_STATE_FILE:-/var/lib/forsteri/install.state}

state_dir() {
  dirname "$FORSTERI_STATE_FILE"
}

state_init() {
  install -d -m 700 "$(state_dir)" 2>/dev/null || mkdir -p "$(state_dir)"
}

# Last value wins; a torn final line is ignored by the reader.
state_set() {
  local key=$1 value=$2 tmp
  [[ $key =~ ^[a-z_]+$ ]] || return 1
  [[ $value != *$'\n'* ]] || return 1
  state_init
  tmp="$FORSTERI_STATE_FILE.tmp.$$"
  {
    if [[ -f $FORSTERI_STATE_FILE ]]; then
      grep -v "^$key=" "$FORSTERI_STATE_FILE" || true
    fi
    printf '%s=%s\n' "$key" "$value"
  } >"$tmp"
  chmod 600 "$tmp" 2>/dev/null || true
  mv -f "$tmp" "$FORSTERI_STATE_FILE"
}

state_get() {
  local key=$1
  [[ -f $FORSTERI_STATE_FILE ]] || return 0
  awk -F= -v k="$key" '$1 == k { value = substr($0, index($0, "=") + 1) } END { if (value != "") print value }' \
    "$FORSTERI_STATE_FILE"
}

state_clear() {
  rm -f "$FORSTERI_STATE_FILE"
}

state_phase() {
  state_get phase
}

# Numeric ordering for resuming. Unknown phases rank 0, which makes the
# installer redo the phase rather than skip past it.
phase_rank() {
  case ${1:-} in
    partitioned) echo 1 ;;
    formatted) echo 2 ;;
    mounted) echo 3 ;;
    base-installing) echo 4 ;;
    base-installed) echo 5 ;;
    configured) echo 6 ;;
    complete) echo 7 ;;
    *) echo 0 ;;
  esac
}

state_reached() {
  local wanted=$1
  (( $(phase_rank "$(state_phase)") >= $(phase_rank "$wanted") ))
}

state_same_disk() {
  [[ -n ${1:-} && $(state_get disk) == "$1" ]]
}
