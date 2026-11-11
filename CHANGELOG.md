# Changelog

What changed for people using Startline, compared with the version before. A release's notes on GitHub are its section here.

## 0.0.1

### Start menu
- The first release: a Start menu for Windows 10 and 11 that the Start key and the Start button open instead of Windows's own, while Shift + Start still opens that one
- Pinned apps, files, folders, web addresses and programs with arguments, in sections of your own, plus Recent files, Most used and All apps
- Five looks: Windows 11, Windows 10, Windows 7, Windows XP, and Startline's own, which can sit anywhere on screen
- Presets that set a look in one go, colour presets or your own colours, and Mica behind the menu on Windows 11
- Start with Windows, on by default, and a tray icon that gives the Windows menu back when Startline quits

### Search
- Apps and settings answered as you type, and files from an index saved to disk, so search works from the first moment
- A search language: exact text, wildcards, words to leave out, file types, kinds, sizes, dates and regular expressions, listed behind the ? in the search box
- Filter chips for folders, documents, pictures and each file type in the results
- Files of the same name show the folder they're in
- Choose where search looks and what it never finds, which kinds of results show, in which order and how many
- Cut, Copy, Paste and Select all in every text box, by keyboard and on a right click, with the Windows clipboard

### Settings in the search
- Type what you want, such as "dark mode", "volume 40" or "bluetooth off", and change it right from the result: dozens of Windows switches and sliders

### Gentle on disks and networks
- Startline reads only the names, sizes and dates of files and folders, never what's in them
- The PC's own drives are indexed; network drives and USB drives each have a switch, off by default, and a disk in a USB enclosure counts as USB
- Reading runs in Windows's background mode, one folder at a time on hard disks, and waits while a full-screen app is in front and for the first ten minutes on battery
- After the first run, the saved index and Windows's change notices keep it fresh, without reading every drive again at each start
- Network shares are read at most once a day while the PC is idle, at most 20 folders a second, with no watch on the server

### Languages
- English, German and Italian, following the language Windows is shown in, or chosen in Settings

### For IT departments
- Every setting can be fixed or given as a default by Group Policy or the Registry, for the PC or one account, with ADMX templates and ADML in English, German and Italian
- Fixed settings show greyed out with "Managed by your organization", and running copies pick up changes within a minute
- An MSI and a setup.exe that install for the PC or one account, silently if you like, and a portable program

### Updates
- Updates from GitHub releases, checked at start and every 6 hours and installed the way Startline was installed, only when signed with Startline's key
