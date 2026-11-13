# Changelog

What changed for people using Startline, compared with the version before. A release's notes on GitHub are its section here.

## Unreleased

- A Best match at the top of the results: the one result whose name fits what you typed best, whatever its kind, so typing a folder's name and pressing Enter opens that folder. It can be turned off, also by policy
- Web searches open your default browser with the search engine you set up there, instead of a fixed address. A fixed address is still there as a setting and a policy
- Newly installed apps show up in the list and in search within seconds, and uninstalled ones go, Store apps included, without restarting Startline
- Microsoft 365, optional and off until you turn it on: sign in with a work or school account on the new Microsoft 365 page of Settings, through Windows's own account sign-in
- Today's calendar beside the menu in the Windows 11 and Startline looks: a timeline with the time now, all-day events on top, the day before and after, Join for online meetings, Accept, Tentative and Decline for invitations, and new events. It shows the last copy at once and while offline
- People in search: colleagues from your organization, with Enter starting a Teams chat and buttons to call or mail them
- Mail in search, by subject, opening the message in Outlook on the web
- Startline asks Microsoft only for what's switched on, talks to nothing but Microsoft's sign-in and Graph, and signing out forgets the account, its tokens and the cached calendar
- Where an organization doesn't let people approve apps themselves, Startline says so and copies the approval link for the administrator. Organizations can switch the account and each part on or off by policy
- An organization's own app registration and tenant can be given by Group Policy, when installing (M365CLIENTID and M365TENANTID for the MSI and setup.exe, kept by upgrades), or when building; Settings says which one is in use

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
