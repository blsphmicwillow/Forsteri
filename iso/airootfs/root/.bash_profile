#
# Forsteri live medium: tty1 starts the installer. Any other tty, or
# FORSTERI_NO_INSTALL=1, gives a plain root shell.
#
[[ -f ~/.bashrc ]] && . ~/.bashrc

if [[ $(tty) == /dev/tty1 && -z ${FORSTERI_NO_INSTALL:-} &&
  ! "$(cat /proc/cmdline 2>/dev/null)" =~ (^| )forsteri\.install_reboot_test=1( |$) &&
  ! "$(cat /proc/cmdline 2>/dev/null)" =~ (^| )forsteri\.install_e2e=1( |$) &&
  -x /usr/local/bin/forsteri-install ]]; then
  /usr/local/bin/forsteri-install
  clear
  echo "Leaving the installer. Run forsteri-install to start again."
fi
