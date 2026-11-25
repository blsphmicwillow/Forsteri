#!/usr/bin/env bash
# Forsteri installer: crash-safe progress state.
#
# The live medium keeps one small file while the installer runs; once the
# target root is mounted the same file is attached to the target. The target
# copy is what makes a resume survive a reboot: archiso's live root is tmpfs.
# The file is intentionally plain text and never sourced as shell code.
#
# The target copy lives at <root>/.forsteri-install.state. UEFI installations
# also mirror it to <root>/efi/.forsteri-install.state so the state can be
# found before the root filesystem is mounted again.

FORSTERI_STATE_FILE=${FORSTERI_STATE_FILE:-/var/lib/forsteri/install.state}
FORSTERI_STATE_MIRROR=${FORSTERI_STATE_MIRROR:-}
STATE_SCAN_FILE=${STATE_SCAN_FILE:-}
STATE_SCAN_PART=${STATE_SCAN_PART:-}
STATE_SCAN_TMPDIR=${STATE_SCAN_TMPDIR:-}

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
    if [[ -n $FORSTERI_STATE_MIRROR && $FORSTERI_STATE_MIRROR != "$FORSTERI_STATE_FILE" ]]; then
      cp -f "$FORSTERI_STATE_FILE" "$FORSTERI_STATE_MIRROR" 2>/dev/null || true
    fi
}

state_get_file() {
  local file=$1 key=$2
  [[ -f $file ]] || return 0
  awk -F= -v k="$key" '$1 == k { value = substr($0, index($0, "=") + 1) } END { if (value != "") print value }' \
    "$file"
}

state_get() {
  state_get_file "$FORSTERI_STATE_FILE" "$1"
}

# Move the authoritative state onto the mounted target. Existing target state
# wins: it is what an earlier boot wrote, and the live copy may be newer only
# because the user just resumed it. The EFI mirror is best-effort.
state_attach_target() {
  local root=$1 target="$1/.forsteri-install.state"
  local efi="$1/efi/.forsteri-install.state"

  if [[ ! -f $target && -f $FORSTERI_STATE_FILE ]]; then
    state_init
    cp -f "$FORSTERI_STATE_FILE" "$target" || return 1
    chmod 600 "$target" 2>/dev/null || true
  fi
  [[ -f $target ]] || return 1
  FORSTERI_STATE_FILE=$target
  if [[ -d $root/efi ]]; then
    FORSTERI_STATE_MIRROR=$efi
    cp -f "$target" "$efi" 2>/dev/null || true
    chmod 600 "$efi" 2>/dev/null || true
  else
    FORSTERI_STATE_MIRROR=""
  fi
}

state_clear() {
  rm -f "$FORSTERI_STATE_FILE"
  if [[ -n $FORSTERI_STATE_MIRROR ]]; then
    rm -f "$FORSTERI_STATE_MIRROR"
  fi
}

# Find a state file on an unmounted target disk, read-only. Returns 0 and sets
# STATE_SCAN_FILE/STATE_SCAN_PART when a state file names this exact disk. The
# caller must copy/import it; this function never changes the live state.
state_scan_disk() {
  local disk=$1 part type mp candidate phase rank best_rank=-1
  local found_file="" found_part="" tmpdir

  STATE_SCAN_FILE=""
  STATE_SCAN_PART=""
  STATE_SCAN_TMPDIR=""
  [[ -b $disk ]] || return 1
  tmpdir=$(mktemp -d) || return 1

  while IFS= read -r part; do
    [[ -n $part && $part != "$disk" ]] || continue
    type=$(blkid -s TYPE -o value "$part" 2>/dev/null || true)
    case $type in
      ext2 | ext3 | ext4 | btrfs | vfat | fat) ;;
      *) continue ;;
    esac
    mp=$(mktemp -d "$tmpdir/mount.XXXXXX") || continue
    if mount -o ro "$part" "$mp" 2>/dev/null; then
      for candidate in "$mp/.forsteri-install.state" "$mp/var/lib/forsteri/install.state"; do
        [[ -f $candidate ]] || continue
        [[ $(state_get_file "$candidate" disk) == "$disk" ]] || continue
        phase=$(state_get_file "$candidate" phase)
        [[ -n $phase ]] || continue
        rank=$(phase_rank "$phase")
        if (( rank > best_rank )); then
          best_rank=$rank
          found_file="$tmpdir/found.state"
          cp -f "$candidate" "$found_file" || continue
          found_part=$part
          STATE_SCAN_TMPDIR=$tmpdir
        fi
      done
      umount "$mp" 2>/dev/null || true
    fi
    rmdir "$mp" 2>/dev/null || true
  done < <(lsblk -lnpo NAME "$disk" 2>/dev/null || true)

  if [[ -n $found_file ]]; then
    STATE_SCAN_FILE=$found_file
    STATE_SCAN_PART=$found_part
    STATE_SCAN_TMPDIR=$tmpdir
    return 0
  fi
  rmdir "$tmpdir" 2>/dev/null || true
  return 1
}

state_import_scan() {
  [[ -n $STATE_SCAN_FILE && -f $STATE_SCAN_FILE ]] || return 1
  state_init
  cp -f "$STATE_SCAN_FILE" "$FORSTERI_STATE_FILE" || return 1
  chmod 600 "$FORSTERI_STATE_FILE" 2>/dev/null || true
  return 0
}

state_scan_cleanup() {
  [[ -n $STATE_SCAN_TMPDIR ]] && rm -rf "$STATE_SCAN_TMPDIR"
  STATE_SCAN_FILE=""
  STATE_SCAN_PART=""
  STATE_SCAN_TMPDIR=""
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
