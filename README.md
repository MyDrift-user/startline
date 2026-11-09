<div align="center">

<img src="assets/logo.png" width="128" alt="Startline logo">

# Startline

**A Start menu for Windows that's yours to arrange, with search that answers as you type.**

Native, written in Rust, with no web view and no runtime to install.

[![CI](https://github.com/MyDrift-user/startline/actions/workflows/ci.yml/badge.svg)](https://github.com/MyDrift-user/startline/actions/workflows/ci.yml)
![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-4CA3FF)
![Rust](https://img.shields.io/badge/rust-2024-CE422B?logo=rust&logoColor=white)
![UI](https://img.shields.io/badge/UI-egui-4C6EF5)
![License](https://img.shields.io/badge/license-GPL--3.0-blue)

<img src="assets/screenshot.png" width="420" alt="Startline's Start menu with pinned apps and recent files">&nbsp;<img src="assets/screenshot-search.png" width="420" alt="Startline searching files for lisbon">

</div>

---

## ✨ Features

### 🪟 Takes over the Start key
While Startline runs, the Start key and the Start button open it instead of the Windows menu. **Shift + Start** still opens the Windows one, and quitting Startline from its tray icon gives everything back.

### ⚡ Search as you type
- Apps and settings are answered in the same frame as the key press
- Files come from an index that keeps every name in one block of memory, so a million files is one fast scan. It's saved to disk, so search works from the first moment, and kept fresh by watching for changes
- Filter chips for folders, documents, pictures and each file type in the results, with the time the search took
- A search language: `"exact text"`, wildcards (`*.pdf`, `img_??.png`), `-word` to leave out, `ext:pdf,docx`, `kind:picture`, `size:>10mb`, `modified:today` or `date:2025-03`, and regular expressions with `re:` or `/…/`, or a switch that makes the whole search one. The `?` in the search box lists it all
- Choose where to search, including network and USB drives as they're attached, and what's never found: ready-made groups, hidden and system files, folders, file types and regular expressions
- Choose which kinds of results show, in which order and how many of each, and hide the parts of the search box you don't use
- A web search for what you typed

### 🎚️ Settings right in the search
Type what you want and act on it from the result: "dark mode", "volume 40", "louder", "bluetooth off", "wlan aus". There are dozens of switches and sliders, from dark mode, Wi-Fi, Bluetooth, volume and brightness to file extensions, hidden files, the clock's seconds, Game Mode and keeping the PC awake. Nothing changes until you act on the row.

### 🎨 Make it yours
- Five looks: Windows 11, Windows 10, Windows 7, Windows XP, or Startline's own, which you can place anywhere on screen
- Presets that set a look in one go: like Windows 11 Start, Startline's own, or compact. Each setting stays yours to change afterwards
- Mica behind the menu on Windows 11 (22H2 and later), with a lighter Mica through the search box and the selected item if you like
- English, German and Italian, following the language Windows is shown in, or chosen in Settings
- Colour presets (Stone, Graphite, Moss, Harbor, Clay, Black, Daylight and Paper) or your own colours
- Pin apps, files, folders, web addresses and programs with arguments, in the order you like
- Choose which sections the menu shows (Pinned, Recent files, Most used, All apps) and how many
- Size, position, corners and transparency
- Start with Windows, on by default

### 🏢 Ready for IT departments
Every setting can be fixed, or given as a default people can change, by **Group Policy** or straight in the Registry (`Software\Policies\Startline` and its `Recommended` subkey, under HKLM or HKCU), for computers or users. ADMX templates come with ADML in English, German and Italian. Pins and their sections, the layout and size, what the menu shows, how search behaves and whether updates install by themselves can all be deployed this way, without copying `start2.bin` around. Fixed settings show greyed out with "Managed by your organization", and running copies pick up changes within a minute. [`PolicyDefinitions/README.txt`](PolicyDefinitions/README.txt) lists every value.

## ⌨️ Keyboard

| Keys | Action |
|---|---|
| `Start` | Open or close Startline |
| `Shift + Start` | The Windows Start menu |
| Just type | Search right away |
| `Enter` | Open, or apply a setting |
| `Ctrl + Enter` | Run as administrator |
| `Shift + Enter` | Show in its folder |
| `Down`, then `Left` and `Right` | Change a selected setting |
| `.pdf` | Only one kind of file |
| `Esc` | Clear, go back, then close |

---

## 📦 Install

Download what you need from **[Releases](../../releases)**, for Windows 10 or 11 (x64):

| File | What it is |
|---|---|
| `...-windows-x64-setup.exe` | Installer, for this PC or just your account |
| `...-windows-x64.msi` | MSI installer with a setup wizard, or silent for managed deployment |
| `...-windows-x64-portable.exe` | Portable, runs without installing |
| `...-windows-x64.zip` | The same portable program, zipped |
| `...-policy-definitions.zip` | Group Policy templates |

Both installers can install for this PC or for just your account. To install without any screens:

```bat
:: For this PC, from an administrator prompt. The folder is optional.
msiexec /i startline-windows-x64.msi /qn ALLUSERS=1 APPLICATIONFOLDER="D:\Apps\Startline"
startline-windows-x64-setup.exe /VERYSILENT /ALLUSERS /DIR="D:\Apps\Startline"

:: For one account, without admin rights.
msiexec /i startline-windows-x64.msi /qn ALLUSERS=2 MSIINSTALLPERUSER=1
startline-windows-x64-setup.exe /VERYSILENT /CURRENTUSER
```

Nothing is signed by a known publisher, so SmartScreen may show a warning. Uninstalling closes Startline and removes its Start with Windows entry.

## 🔄 Updates

Startline looks for a new release 90 seconds after it starts and every 6 hours, and installs it by itself unless you turn that off under **Settings > About & updates**, which also shows the version and lets you check by hand. As Startline is always running, it closes and starts again with the new version, only while neither the menu nor the settings window is open:

| Installed as | How updates land |
|---|---|
| For one account (MSI or `setup.exe`) | Installed silently, then Startline starts again |
| For this PC (MSI or `setup.exe`) | A scheduled task, `Startline Updater`, installs it as SYSTEM within the hour and starts Startline again for everyone who had it running. No admin prompts. The install folder is locked so only administrators can change it |
| Portable | Replaces its own file and starts the new one |

Every release lists its files' checksums in `latest.json`, signed in `latest.json.sig`. Startline checks that signature against a key built into it, and refuses anything that doesn't match. Administrators can fix automatic updates on or off with the **AutomaticUpdates** policy, or stop Startline looking for updates at all with **CheckForUpdates**. `startline.exe --update-now` does an update check from a script, and what happened is logged to `update.log` in the settings folder.

## ⚙️ Settings and data

Settings, the app list, the search index and the update log are in `%LOCALAPPDATA%\Startline\`. Settings are a JSON file, `config.json`, and survive updates.

## 🛠️ Build from source

You need [Rust](https://rustup.rs).

```sh
cargo build --release
```

The program ends up in `target/release/startline.exe`. Its icon and version need a resource compiler: `rc.exe` from the Windows SDK with MSVC, or `windres` (in MSYS2) on the PATH with the GNU toolchain. With the `x86_64-pc-windows-gnu` toolchain, the `windows` crate needs a working `dlltool`, for example MSYS2's, set with `-C dlltool=...` in `.cargo/config.toml`. The MSVC toolchain needs nothing extra.

`assets/make-assets.ps1` draws the logo, the icon and the installer's pictures from the same shape as the app's icon.

## 🚀 Releasing

Every push is built and tested by the CI workflow, which also checks that `PolicyDefinitions` matches what the program writes with `startline.exe --write-policy-templates PolicyDefinitions`. Releases only start by hand:

1. Raise `version` in `Cargo.toml` and push. A version can't change once it's released, because updates go by the number.
2. Go to **Actions > Release > Run workflow**. Turning off **Publish** only builds and tests, leaving the files on the run.

Publishing waits for an approval on GitHub, as the signing key is a secret of the protected `release` environment. If that key is lost or leaks, a release signed with the offline backup key can still reach every copy, and replace it.

The workflow builds and tests the program, builds every download, checks that both installers install and uninstall (for this PC, for one account and into a chosen folder) and that the MSI upgrades the published release. It then updates every kind of install from this version to a newer test build, through the real installers and the real SYSTEM task, signed with a throwaway key, and checks that bad signatures, changed files and the update policies stop it. Last, it signs the update list, checks the program accepts it, tags `v<version>` and publishes a GitHub Release.

## 🗺️ Project layout

```
src/
  main.rs                 entry point and the app's icon
  ui.rs, popup.rs         the window and how it opens and closes
  home.rs, classic.rs     the Start menu, in Startline's look and the older Windows looks
  search.rs, search_page.rs, matching.rs   search and its results
  index.rs, scope.rs      the file index and where it looks
  apps.rs, icons.rs       installed apps and their icons
  intent.rs, settings.rs, actions.rs   settings you can change from a search
  system.rs, platform.rs  the Start key, tray icon and Windows calls
  customize.rs, pins_page.rs, setting_list.rs, widgets.rs, styles.rs   Settings and the looks
  config.rs               what's saved
  i18n/                   English, German and Italian, with the house style and glossary in its README
  policy*.rs              Group Policy: the list, reading it, the templates and admin guide, and their tests
  update.rs               signed updates from GitHub releases
build.rs                  puts the icon and version into startline.exe
PolicyDefinitions/        ADMX / ADML templates
assets/                   logo, icon, screenshots and the script that draws them
installer/windows/        setup.exe (Inno Setup) and MSI (WiX) definitions, packaging, and the installer and update tests
```

## 📄 License

Copyright (C) 2026 MyDrift. Startline is free software, released under the [GNU General Public License v3.0](LICENSE).

---

<div align="center">
<sub>Made with 🦀 Rust and <a href="https://github.com/emilk/egui">egui</a>.</sub>
</div>
