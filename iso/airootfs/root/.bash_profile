#
# Forsteri live medium: tty1 starts the installer. Any other tty, or
# FORSTERI_NO_INSTALL=1, gives a plain root shell.
#
[[ -f ~/.bashrc ]] && . ~/.bashrc

if [[ $(tty) == /dev/tty1 && -z ${FORSTERI_NO_INSTALL:-} && -x /usr/local/bin/forsteri-install ]]; then
  /usr/local/bin/forsteri-install
  clear
  echo "Leaving the installer. Run forsteri-install to start again."
fi
