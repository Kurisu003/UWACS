# UWACS: Universal Winwing Accessories Control System

One Rust program that replaces the old UWACS Tauri app and its three child programs
(ufc_writer, pfp_writer, FluidPort). Linux first (tested on Arch + KDE Plasma Wayland).

## What it does
- **Devices**: lists connected WinWing HID panels (vendor 0x4098) with live button state and
  USB/udev permission status, plus WinWing DisplayLink instrument displays and their desktop output.
- **Panels**: the UFC writer (F/A-18C, AV-8B) and the PFP writer (A-10C II CDU, AH-64D, AV-8B,
  CH-47F, RWR search mode on MENU, PFP keys to DCS-BIOS). Same packets and tables as before.
- **Remap**: map any button on any panel to a keyboard shortcut (virtual uinput keyboard, held while
  the button is held) or to UWACS actions (overlay on/off, overlay next monitor). Learn by pressing.
- **Displays**: drag monitors to arrange them, rotate or flip them (kscreen-doctor), 15 s auto-revert.
- **Overlay**: FluidPort's always-on-top click-through image (AH-64D MFD frame by default), placed by KWin.
- **DCS setup**: DCS-BIOS host/port and a MonitorSetup Lua generator (FluidPort's mine.lua values).

## Build and run
    cargo build --release
    ./target/release/uwacs            # window
    ./target/release/uwacs --list     # print devices and monitors
    ./target/release/uwacs --headless # writers + remapping without a window

Config: `~/.config/uwacs/config.json` (override with `UWACS_CONFIG`).

## Linux permissions
- Panels: udev rule (the Devices tab shows it and copies the install command):
  `KERNEL=="hidraw*", ATTRS{idVendor}=="4098", MODE="0660", TAG+="uaccess"`
- Remapping: your user must be in the `input` group (for /dev/uinput).
- Instrument displays: AUR `evdi-dkms` + `displaylink`, `systemctl enable --now displaylink`.

## Tests
    cargo test                                   # protocol, parser, segment and packet tests
    cargo test live_descriptors -- --ignored --nocapture   # parse this PC's HID descriptors
    UWACS_TAB=remap UWACS_SCREENSHOT=out.png uwacs           # screenshot a tab and quit

`legacy/` holds untouched copies of original files for reference.

## v0.3 additions

- **Button mapping pictures**: every connected panel is drawn (`src/devicemaps.rs`, own schematic drawings), one per
  device, live-lit per instance. Click a button to record a shortcut. Dashed buttons have no known HID number yet:
  click, press the real button once, and the number is saved in `button_overrides` in the config.
  Button numbers and USB PIDs come from dcs-signal-converter's `data/devices/winctrl.json` (MIT, Cory Bass).
- **Overlays**: several at once (`overlays` in the config), each on its own screen. Position is the picture's
  top-left corner from the screen's top-left. Fields accept sums (`778/2`, `W-747`, `(H-762)/2`), see `src/calc.rs`.
- **Monitor setup follows overlays**: MFD n goes where overlay n is; for the AH-64D the export is shrunk into the
  frame's see-through window (found from the picture's transparent centre, `overlay::detect_hole`).
- **One-click DCS setup** (DCS setup page, or `uwacs --dcs-setup FOLDER`): finds Saved Games/DCS from the prefix,
  game folder or Saved Games folder, installs the newest DCS-BIOS release, adds the Export.lua line (backups kept),
  writes the monitor setup.

## v0.4 additions

- **Custom panel screens** (Panel screens page): your own UFC and PFP pages per aircraft (or for every aircraft).
  Write text with DCS-BIOS names in curly brackets, e.g. `FUEL {IFEI_FUEL_INT}`; `{NAME%}` = percent of maximum,
  `{NAME:5}` = exactly 5 characters. Names and live values come from the installed DCS-BIOS
  (`Scripts/DCS-BIOS/doc/json`), browsable in the "DCS-BIOS names" list. A page for the exact aircraft beats an
  every-aircraft page, and both beat the built-in pages.
- **Automatic updates** (Settings page, on by default, can be switched off): checks GitHub releases 20 s after start
  and every 6 hours. A release needs a `vX.Y.Z` tag and the asset `uwacs-linux-x86_64` (built by
  `.github/workflows/release.yml` when a tag is pushed). The download must answer `--version` correctly before it
  replaces the running binary; the previous one stays as `uwacs.old`. `uwacs --update` does the same from a terminal.
- **Export everything / Import** (Settings page): one JSON file with every setting; importing backs up the current
  settings first.
- **Profiles**: profile switches (e.g. the MFD side switch 45/46/47) select which mappings are active; a mapping only
  works in the profile it was made in.

Releasing: bump `version` in Cargo.toml, commit, `git tag vX.Y.Z && git push --tags`.
