# Changelog

What changed for people using Startline, compared with the version before. A release's notes on GitHub are its section here.

## 0.0.2

Released 2026-11-14.

Search finds more and finds it better: a Best match first, stricter matching, bookmarks, a calculator, your clipboard history and quick switches, all in your language. The menu gets a Now playing card, a theme that follows Windows' light and dark mode, Mica that works on every Windows 11 PC, and an optional Microsoft 365 account for your calendar, colleagues and mail. Clicking the taskbar's search button now opens Startline's search and keeps it open, and the menu opens next to the taskbar, wherever it is.

### Start menu
- The menu opens where Windows' own Start menu would, in every look: against the taskbar on whichever edge it is (bottom, top, left or right), at its start or in its middle as the taskbar's alignment in Windows says, on the monitor whose taskbar or Start button you used. It follows the taskbar when it moves, its alignment changes, it hides itself, or monitors and display sizes change, without a restart
- Under Settings > Start menu, Position is now "Align to taskbar" or "Custom": Custom keeps the spot on the screen you pick, in every look, center of the screen included. If you had picked a spot before, it stays; if you never changed it, the menu now follows the taskbar
- A new policy, AlignToTaskbar, as a fixed setting or a default
- Now playing, on at first: what plays in Spotify, a browser or any player that tells Windows, with its cover, title, artist and app, play or pause, previous and next, and a switch between apps when several play. Out of sight when nothing plays; the cover can be switched off
- A new color theme, System, now the default for the Startline look: it follows Windows' light or dark mode for apps as soon as it changes, with a light and a dark theme you pick. Themes chosen before stay as they were
- Mica works on every Windows 11 PC: the menu paints it itself from your desktop picture, blurred and tinted, instead of a plain black or white sheet that hid the menu
- Scroll bars show while you scroll and whenever a list isn't at its top, and fade away at the top, as in Windows 11, instead of whenever the pointer is over the menu
- Only one copy of Startline runs per user session, however it's started; a second one opens the running one's menu and ends
- Clicking the search button or box on the taskbar opens Startline's search and keeps it open (#2), on every taskbar, without Windows Search starting underneath

### Search
- A Best match at the top: the one result whose name fits what you typed best, whatever its kind. It can be turned off, also by policy
- Stricter matching everywhere: a name is found by a piece of it, the start of a word, or the first letters of its words ("vsc" for Visual Studio Code), but no longer by letters scattered through it, so "wifi" doesn't find "Windows-Fotoanzeige"
- Web searches open your default browser with its own search engine, show that browser's icon, and their name is in your language. A fixed address is still there as a setting and a policy
- Newly installed apps show up within seconds, and uninstalled ones go, Store apps included
- Bookmarks, off until you turn them on: every profile's bookmarks from Chrome, Edge, Brave, Vivaldi, Opera, Helium and other Chromium browsers and from Firefox, with each site's own icon, only ever read and opening in the browser and profile they're from; your own bookmarks of web addresses, files, folders and commands, in folders, added in Settings or from a result's right-click menu; and your organization's
- A calculator, off until you turn it on: sums, conversions of length, weight, temperature, volume, data sizes and time, and dates such as today + 30 days. Enter copies the result
- Clipboard history, off until you turn it on: find what you copied by its words, from three letters on, and copy it again with Enter. At most three show in an ordinary search, below apps, settings and bookmarks; start with "clipboard" for more
- Do not disturb and switching the sound to another output join the Windows settings search can change; Wi-Fi and Bluetooth only show on PCs that have them; power actions in search get a switch of their own
- The keyboard hint under the results fits the result: Enter starts a Teams chat for a person, opens a message, copies a result or opens a bookmark

### Microsoft 365
- Optional and off until you turn it on: sign in with a work or school account on the new Microsoft 365 page of Settings, through Windows' own account sign-in
- Today's calendar beside the menu in the Windows 11 and Startline looks, with Join for online meetings, Accept, Tentative and Decline for invitations (also from a menu where an invitation is too narrow), and new events. It shows the last copy at once and offline
- People and mail in search: colleagues to chat with, call or mail, and your mail by subject, also for what you typed before the account was ready
- Startline asks Microsoft only for what's switched on, talks to nothing but Microsoft's sign-in and Graph, and signing out forgets everything. Where an organization has to approve Startline first, the page says so and copies the approval link

### For IT departments
- Policies for everything new: MediaControls, MediaCovers, ColorThemeLight, ColorThemeDark and System for ColorTheme, SearchBookmarks, BookmarksFromBrowsers, BookmarksLeftOut, Bookmarks, SearchCalculator, SearchClipboardHistory, SearchPowerActions and the Microsoft 365 ones
- An organization's own Microsoft 365 app registration and tenant can be given by Group Policy, when installing (M365CLIENTID and M365TENANTID for the MSI and setup.exe, kept by upgrades), or when building; Settings says which one is in use

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
