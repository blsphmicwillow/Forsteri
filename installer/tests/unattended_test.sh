#!/usr/bin/env bash
# Unit tests for the unattended answer file. No dialog, no root, no disks.
#
# The installer's --unattended mode replaces only the dialog boxes; these tests
# pin the key mapping and the refusal to guess when an answer is absent.
set -euo pipefail

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

FORSTERI_LOG=$tmp/install.log
FORSTERI_SHARE=$HERE/../../iso/airootfs/usr/share/forsteri
export FORSTERI_LOG FORSTERI_SHARE
# shellcheck source=../../iso/airootfs/usr/lib/forsteri/installer/common.sh
. "$HERE/../../iso/airootfs/usr/lib/forsteri/installer/common.sh"

FORSTERI_ANSWERS=$tmp/answers.conf
cat >"$FORSTERI_ANSWERS" <<'ANSWERS'
menu.disk=/dev/vda
menu.filesystem=ext4
input.erase=ERASE
input.hostname=forsteri-e2e
input.user=forsteri
password.password_for_forsteri=hunter2
password.root_password=root-secret
input.timezone=UTC
menu.locale=en_US.UTF-8
menu.keyboard=us
menu.desktop=hyprland
menu.theme=ember
yesno.install=yes
yesno.resume=no
menu.bogus=made-up
ANSWERS

fail=0
ok() { printf 'ok   %s\n' "$*"; }
bad() {
  printf 'FAIL %s\n' "$*"
  fail=1
}

[[ $(ui_menu "Disk" "text" /dev/vda "4G" /dev/vdb "8G") == /dev/vda ]] &&
  ok "menu answer" || bad "menu answer"
[[ $(ui_menu "Filesystem" "text" ext4 "ext4" btrfs "btrfs") == ext4 ]] &&
  ok "filesystem menu" || bad "filesystem menu"
[[ $(ui_input "Hostname" "text" "default") == forsteri-e2e ]] &&
  ok "input answer overrides default" || bad "input answer"
[[ $(ui_input "Erase /dev/vda" "text" "") == ERASE ]] &&
  ok "title with a path falls back to the first word" || bad "erase fallback"
[[ $(ui_password "Password for forsteri" "text") == hunter2 ]] &&
  ok "password answer" || bad "password answer"

if ui_yesno "Install" "text"; then ok "yes answer"; else bad "yes answer"; fi
if ui_yesno "Resume interrupted install" "text"; then bad "no answer"; else ok "no answer"; fi

if ui_menu "Bogus" "text" gnome "GNOME" hyprland "Hyprland" >/dev/null; then
  bad "unknown menu answer accepted"
else
  ok "unknown menu answer refused"
fi

if ui_input "Secret" "text" "" >/dev/null; then
  bad "missing input was invented"
else
  ok "missing input refused"
fi

if ui_yesno "Unanswered" "text" >/dev/null; then
  bad "missing yes/no was invented"
else
  ok "missing yes/no refused"
fi

# Last value wins, and # comments are not answers.
cat >"$FORSTERI_ANSWERS" <<'ANSWERS'
# input.hostname=wrong
input.hostname=first
input.hostname=second
ANSWERS
[[ $(ui_input "Hostname" "text" "default") == second ]] &&
  ok "last duplicated answer wins" || bad "duplicate answer"

# The real UI functions still call dialog when unattended mode is off.
FORSTERI_ANSWERS=""
DIALOG=$tmp/fake-dialog
cat >"$DIALOG" <<'DIALOG'
#!/usr/bin/env bash
# dialog writes its answer to fd 3 after the UI has redirected the streams.
printf 'dialog-called' >&3
DIALOG
chmod +x "$DIALOG"
[[ $(ui_input "Hostname" "text" "default" 2>/dev/null) == dialog-called ]] &&
  ok "interactive path still uses dialog" || bad "interactive path"

if (( fail == 0 )); then
  echo "unattended answer tests passed"
else
  echo "unattended answer tests failed"
fi
exit "$fail"
