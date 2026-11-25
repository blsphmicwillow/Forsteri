#!/usr/bin/env bash
# Forsteri installer: disk selection, partitioning, formatting, mounting.

pick_disk() {
  local tag size model items=()
  while read -r tag size model; do
    [[ -z $tag ]] && continue
    items+=("$tag" "${size} ${model:-disk}")
  done < <(lsblk --nodeps --paths --noheadings --output NAME,SIZE,MODEL | sort)

  (( ${#items[@]} > 0 )) || die "no disks were found"

  local choice
  choice=$(ui_menu "Disk" \
    "Choose the disk Forsteri will be installed on.

Everything on the selected disk is destroyed." "${items[@]}") || return 1
  CFG[disk]=$choice
  assert_disk_is_safe

  # Firmware is a property of the machine, not a question: detect it once here.
  if is_uefi; then
    CFG[firmware]=uefi
  else
    CFG[firmware]=bios
  fi
  log "firmware: ${CFG[firmware]}"
}

fs_type() {
  local part=$1
  [[ -b $part ]] || return 0
  blkid -s TYPE -o value "$part" 2>/dev/null || true
}

confirm_wipe() {
  local typed
  typed=$(ui_input "Erase ${CFG[disk]}" \
    "This erases every partition and filesystem on ${CFG[disk]}.

Type ERASE to continue." "") || return 1
  [[ ${typed^^} == "ERASE" ]]
}

partition_disk() {
  local disk=${CFG[disk]}
  step "partitioning $disk (${CFG[firmware]})"

  run_logged "wipefs" wipefs --all --force "$disk" || die "could not wipe $disk"
  run_logged "sgdisk zap" sgdisk --zap-all "$disk" || die "could not clear $disk"

  if [[ ${CFG[firmware]} == uefi ]]; then
    sgdisk --new=1:0:+512M --typecode=1:ef00 --change-name=1:FORSTERI-EFI "$disk" >>"$FORSTERI_LOG" 2>&1 ||
      die "could not create the EFI partition"
    CFG[efipart]=$(part_path "$disk" 1)
  else
    sgdisk --new=1:0:+2M --typecode=1:ef02 --change-name=1:FORSTERI-BOOT "$disk" >>"$FORSTERI_LOG" 2>&1 ||
      die "could not create the BIOS boot partition"
    CFG[efipart]=""
  fi

  sgdisk --new=2:0:0 --typecode=2:8304 --change-name=2:FORSTERI-ROOT "$disk" >>"$FORSTERI_LOG" 2>&1 ||
    die "could not create the root partition"
  CFG[rootpart]=$(part_path "$disk" 2)

  run_logged "partprobe" partprobe "$disk"
  udevadm settle --timeout=10 || true
  log "root partition: ${CFG[rootpart]}, efi: ${CFG[efipart]:-none}"
}

format_partitions() {
  step "formatting"
  if [[ -n ${CFG[efipart]} ]]; then
    run_logged "mkfs.fat" mkfs.fat -F 32 -n FORSTERI-EFI "${CFG[efipart]}" ||
      die "could not format ${CFG[efipart]}"
  fi

  case ${CFG[fs]} in
    ext4)
      run_logged "mkfs.ext4" mkfs.ext4 -F -L Forsteri -m 1 "${CFG[rootpart]}" ||
        die "could not format ${CFG[rootpart]}"
      ;;
    btrfs)
      run_logged "mkfs.btrfs" mkfs.btrfs -f -L Forsteri "${CFG[rootpart]}" ||
        die "could not format ${CFG[rootpart]}"
      ;;
    *)
      die "unsupported filesystem ${CFG[fs]}"
      ;;
  esac
}

mount_target() {
  step "mounting"
  if mountpoint -q "$TARGET"; then
    local source
    source=$(findmnt -n -o SOURCE "$TARGET" 2>/dev/null || true)
    [[ $source == "${CFG[rootpart]}" ]] ||
      die "$TARGET is already mounted from ${source:-somewhere else}"
    step "resume: $TARGET is already mounted"
  else
    mount "${CFG[rootpart]}" "$TARGET" || die "could not mount ${CFG[rootpart]}"
  fi
  if [[ -n ${CFG[efipart]} ]]; then
    mkdir -p "$TARGET/efi"
    if mountpoint -q "$TARGET/efi"; then
      step "resume: $TARGET/efi is already mounted"
    else
      mount "${CFG[efipart]}" "$TARGET/efi" || die "could not mount ${CFG[efipart]}"
    fi
  fi
}

unmount_target() {
  if mountpoint -q "$TARGET/efi" 2>/dev/null; then
    umount -R "$TARGET/efi" || true
  fi
  if mountpoint -q "$TARGET" 2>/dev/null; then
    umount -R "$TARGET" || true
  fi
}
