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

# Set by `forsteri-install --unattended FILE`. When it is non-empty the ui_*
# functions read answers from that file instead of drawing dialog boxes; every
# other line of the install flow is the same code. `FORSTERI_UNATTENDED_CONSOLE=1`
# also copies log lines to stderr so a serial console sees them.
FORSTERI_ANSWERS=${FORSTERI_ANSWERS:-}
FORSTERI_UNATTENDED_CONSOLE=${FORSTERI_UNATTENDED_CONSOLE:-0}

# ---------------------------------------------------------------------------
# logging and UI
# ---------------------------------------------------------------------------

log() {
  local line
  line="$(date -u +%FT%TZ) $*"
  printf '%s\n' "$line" >>"$FORSTERI_LOG" 2>/dev/null || true
  if [[ $FORSTERI_UNATTENDED_CONSOLE == 1 ]]; then
    printf '%s\n' "$line" >&2
  fi
}

step() {
  log "== $*"
}

# ---------------------------------------------------------------------------
# unattended answers
# ---------------------------------------------------------------------------

unattended() {
  [[ -n $FORSTERI_ANSWERS ]]
}

# A prompt title becomes a stable key: lower case, runs of non-alphanumerics
# folded to one underscore. "Password for forsteri" -> password_for_forsteri.
answer_slug() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '_' |
    sed 's/^_//; s/_$//'
}

answer_raw() {
  local key=$1
  [[ -r $FORSTERI_ANSWERS ]] || return 1
  awk -F= -v k="$key" '
    /^[[:space:]]*#/ { next }
    $1 == k { value = substr($0, index($0, "=") + 1) }
    END { if (value != "") print value }
  ' "$FORSTERI_ANSWERS"
}

# ui_answer <kind> <title>. Exact key first, then the first word so a title that
# carries a disk path ("Erase /dev/vda") can still use a fixed answer.
ui_answer() {
  local kind=$1 title=$2 slug head value
  slug=$(answer_slug "$title")
  value=$(answer_raw "$kind.$slug" || true)
  if [[ -z $value ]]; then
    head=${slug%%_*}
    [[ $head != "$slug" ]] && value=$(answer_raw "$kind.$head" || true)
  fi
  if [[ -z $value ]]; then
    log "unattended: no answer for $kind \"$title\" (tried $kind.$slug)"
    return 1
  fi
  printf '%s\n' "$value"
}

ui_msg() {
  if unattended; then
    log "unattended: message \"$1\""
    [[ $FORSTERI_UNATTENDED_CONSOLE == 1 ]] && printf '\n[%s]\n%s\n' "$1" "$2" >&2
    return 0
  fi
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --msgbox "$2" 0 0
}

ui_info() {
  if unattended; then
    log "unattended: info \"$1\""
    [[ $FORSTERI_UNATTENDED_CONSOLE == 1 ]] && printf '[%s] %s\n' "$1" "$2" >&2
    return 0
  fi
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --infobox "$2" 0 0
}

ui_yesno() {
  local answer
  if unattended; then
    answer=$(ui_answer yesno "$1") || return 1
    log "unattended: yes/no \"$1\" = $answer"
    case ${answer,,} in
      yes | y | true | 1) return 0 ;;
      no | n | false | 0) return 1 ;;
      *)
        log "unattended: \"$answer\" is not yes or no for \"$1\""
        return 1
        ;;
    esac
  fi
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --yesno "$2" 0 0
}

ui_input() {
  local default=${3:-} answer
  if unattended; then
    answer=$(ui_answer input "$1" || true)
    [[ -n $answer ]] || answer=$default
    if [[ -z $answer ]]; then
      log "unattended: \"$1\" has no answer and no default"
      return 1
    fi
    log "unattended: input \"$1\" = $answer"
    printf '%s\n' "$answer"
    return 0
  fi
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --inputbox "$2" 0 0 "$default" 3>&1 1>&2 2>&3
}

ui_password() {
  local answer
  if unattended; then
    answer=$(ui_answer password "$1") || return 1
    log "unattended: password \"$1\" supplied"
    printf '%s\n' "$answer"
    return 0
  fi
  "$DIALOG" --backtitle "$BACKTITLE" --title "$1" --insecure --passwordbox "$2" 0 0 3>&1 1>&2 2>&3
}

ui_menu() {
  local title=$1 text=$2 answer tag found=0
  shift 2
  if unattended; then
    answer=$(ui_answer menu "$title") || return 1
    while (( $# )); do
      tag=$1
      shift 2
      [[ $tag == "$answer" ]] && found=1
    done
    if (( found == 0 )); then
      log "unattended: \"$answer\" is not a choice for \"$title\""
      return 1
    fi
    log "unattended: menu \"$title\" = $answer"
    printf '%s\n' "$answer"
    return 0
  fi
  "$DIALOG" --backtitle "$BACKTITLE" --title "$title" --menu "$text" 0 0 12 "$@" 3>&1 1>&2 2>&3
}

# An interactive prompt declined or answered with nothing. A person may walk
# away from the TUI, but an unattended run must fail loudly instead of
# pretending it installed anything.
cancel() {
  if unattended; then
    die "unattended run cannot continue: an answer was missing or declined"
  fi
  exit 0
}

die() {
  local message=$1
  log "fatal: $message"
  if unattended; then
    printf '\nFORSTERI-INSTALL: UNATTENDED FAIL: %s\n' "$message" >&2
    [[ -f $FORSTERI_LOG ]] && tail -n 80 "$FORSTERI_LOG" >&2 || true
    exit 1
  fi
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
