#!/usr/bin/env bash
# Forsteri installer: shared state, UI and validation.
#
# Sourced by /usr/local/bin/forsteri-install. Every function assumes it runs as
# root on the live medium.

# Overridable so `scripts/check.sh` can lint a staged tree without installing.
readonly FORSTERI_VERSION="0.1 (Ember)"
readonly FORSTERI_LOG="${FORSTERI_LOG:-/var/log/forsteri-install.log}"
readonly FORSTERI_SHARE="${FORSTERI_SHARE:-/usr/share/forsteri}"
readonly TARGET="${FORSTERI_TARGET:-/mnt}"

declare -A CFG=(
  [disk]=""
  [rootpart]=""
  [efipart]=""
  [firmware]=""
  [fs]="ext4"
  [hostname]="forsteri"
  [timezone]="UTC"
  [locale]="en_US.UTF-8"
  [keymap]="us"
  [user]="forsteri"
  [desktop]="minimal"
  [theme]="ember"
)
ROOT_PASSWORD=""
USER_PASSWORD=""

DIALOG="${DIALOG:-dialog}"
readonly BACKTITLE="Forsteri OS ${FORSTERI_VERSION}"

# ---------------------------------------------------------------------------
# logging and UI
# ---------------------------------------------------------------------------

log() {
  printf '%s %s\n' "$(date -u +%FT%TZ)" "$*" >>"$FORSTERI_LOG" 2>/dev/null || true
}

step() {
  log "== $*"
}

ui_msg() {
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --msgbox "$2" 0 0
}

ui_info() {
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --infobox "$2" 0 0
}

ui_yesno() {
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --yesno "$2" 0 0
}

ui_input() {
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --inputbox "$2" 0 0 "$3" 3>&1 1>&2 2>&3
}

ui_password() {
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --insecure --passwordbox "$2" 0 0 3>&1 1>&2 2>&3
}

ui_menu() {
  local title=$1 text=$2
  shift 2
  "$DIALOG" --backtitle "$BACKTITLE" --title "$title" --menu "$text" 0 0 12 "$@" 3>&1 1>&2 2>&3
}

die() {
  local message=$1
  log "fatal: $message"
  ui_msg "Something went wrong" "$message

Details: $FORSTERI_LOG"
  exit 1
}

# Run a command quietly, keeping its output for the log.
run_logged() {
  local description=$1
  shift
  log "run ($description): $*"
  "$@" >>"$FORSTERI_LOG" 2>&1
}

require_root() {
  (( EUID == 0 )) || die "the installer must run as root"
}

# ---------------------------------------------------------------------------
# data files
# ---------------------------------------------------------------------------

# One package spec per line, `#` comments and whitespace removed.
read_spec_list() {
  local file=$1 line
  [[ -f $file ]] || die "missing package list: $file"
  while IFS= read -r line; do
    line=${line%%#*}
    line=${line//[[:space:]]/}
    [[ -n $line ]] && printf '%s\n' "$line"
  done <"$file"
}

# ---------------------------------------------------------------------------
# validation
# ---------------------------------------------------------------------------

valid_hostname() {
  local name=$1
  (( ${#name} >= 1 && ${#name} <= 63 )) || return 1
  [[ $name =~ ^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?$ ]]
}

valid_username() {
  local name=$1
  (( ${#name} >= 1 && ${#name} <= 32 )) || return 1
  [[ $name =~ ^[a-z_][a-z0-9_-]*$ ]]
}

valid_timezone() {
  [[ -f "/usr/share/zoneinfo/$1" ]]
}

is_uefi() {
  [[ -d /sys/firmware/efi ]]
}

# Partition path for the first/second partition of a disk.
part_path() {
  local disk=$1 index=$2
  case $disk in
    *nvme* | *mmcblk* | *loop*) printf '%sp%s\n' "$disk" "$index" ;;
    *) printf '%s%s\n' "$disk" "$index" ;;
  esac
}

# Refuse to touch the medium we booted from, or any mounted disk.
assert_disk_is_safe() {
  local disk=${CFG[disk]}
  [[ -b $disk ]] || die "$disk is not a block device"
  if lsblk -nr -o MOUNTPOINT "$disk" 2>/dev/null | grep -qv '^$'; then
    die "$disk has mounted partitions (is this the live medium?); choose another disk"
  fi
  if findmnt --source "$disk" >/dev/null 2>&1; then
    die "$disk is in use; choose another disk"
  fi
}

# ---------------------------------------------------------------------------
# credentials
# ---------------------------------------------------------------------------

ask_password_pair() {
  local title=$1 first second
  first=$(ui_password "$title" "Enter the password.") || return 1
  (( ${#first} >= 4 )) || {
    ui_msg "$title" "The password must be at least 4 characters."
    return 1
  }
  second=$(ui_password "$title" "Enter the password again.") || return 1
  [[ $first == "$second" ]] || {
    ui_msg "$title" "The passwords do not match."
    return 1
  }
  printf '%s\n' "$first"
}
