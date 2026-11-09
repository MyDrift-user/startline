Startline: guide for administrators
===================================

Everything that can be set in Startline's settings can also be set by Group Policy or straight in the Registry, for
the whole PC or for one account: the look, the language, the pinned apps and their sections, the layout and size of
the menu, what it shows, and how search behaves. That makes Startline a way to deploy a Start menu without the usual
tricks, such as copying start2.bin into profiles or importing layout XML: write the values below and every Startline
picks them up, including copies that are already running.

This file is written by Startline from its own list of policies, together with the templates next to it. To write
them all again:
    startline.exe --write-policy-templates <folder>


INSTALLING THE TEMPLATES
Copy Startline.admx to C:\Windows\PolicyDefinitions, and the language folders you need (en-US, de-DE, it-IT) with
their Startline.adml to the folders of the same name under it. For a domain, copy them to the same places in the
Central Store (\\<domain>\SYSVOL\<domain>\Policies\PolicyDefinitions). The editor shows Startline under
Administrative Templates, both under Computer Configuration and under User Configuration.


FIXED SETTINGS AND DEFAULTS
Each setting comes twice:
  Startline                          A fixed setting. It always applies. People see it greyed out in Settings with
                                     "Managed by your organization", and can't change it. Removing the policy brings
                                     back what they had chosen before.
  Startline > Defaults people        A default. It applies until the person changes that setting themselves, and from
  can change                         then on their own choice stays, even if the default changes later.
Where both are set, the fixed one wins. Where the computer and the account both set one, the computer's wins.

Some policies aren't settings but limits on what people can do (the commands, the right-click options and the
Windows settings search can change). They can only be fixed.

Startline reads the policies when it starts and every half minute while it runs, so a gpupdate or a script's change
reaches running copies within a minute.


THE REGISTRY
For Group Policy Preferences, Intune or scripts, write the values yourself:
  Fixed:      HKLM\Software\Policies\Startline             (the whole PC)
              HKCU\Software\Policies\Startline             (one account)
  Defaults:   HKLM\Software\Policies\Startline\Recommended
              HKCU\Software\Policies\Startline\Recommended
Switches and numbers are REG_DWORD, text is REG_SZ (REG_EXPAND_SZ works too). A list is a REG_DWORD of 1 under the
list's name, with its items as REG_SZ values named 1, 2, 3 and so on in a subkey of the same name. A list with the
REG_DWORD of 1 and no items is an empty list. Tick boxes are REG_DWORD values named Allow_<item>, 0 or 1.

For example, a Start menu with two sections of pins, the Startline look, search at the top and German text, fixed
for everyone on the PC:

    reg add HKLM\Software\Policies\Startline /v Experience /t REG_SZ /d Startline /f
    reg add HKLM\Software\Policies\Startline /v Language /t REG_SZ /d de /f
    reg add HKLM\Software\Policies\Startline /v SearchAtBottom /t REG_DWORD /d 0 /f
    reg add HKLM\Software\Policies\Startline /v PinnedApps /t REG_DWORD /d 1 /f
    reg add HKLM\Software\Policies\Startline\PinnedApps /v 1 /t REG_SZ /d "Office|Word|C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE" /f
    reg add HKLM\Software\Policies\Startline\PinnedApps /v 2 /t REG_SZ /d "Office|Intranet|https://intranet.example.com" /f
    reg add HKLM\Software\Policies\Startline\PinnedApps /v 3 /t REG_SZ /d "Tools|Explorer|app:Microsoft.Windows.Explorer" /f

The same under ...\Startline\Recommended makes them the starting point, which people can then rearrange.

Pinned apps are written Section|Name|Target or Section|Name|Target|Arguments. The target is a file, folder, program or
web address, or app:<ID> for an app installed from the Store or with an Application User Model ID. Environment
variables such as %USERPROFILE% and %ProgramFiles% work in targets and arguments.


SEARCH
The search policies are under Startline > Search, in four parts:
  Where to search   the places, and whether network and USB drives are searched as they attach.
  Never found       what search never shows: six ready-made groups (SearchSkipAppData, SearchSkipWindows,
                    SearchSkipDeveloperFiles, SearchSkipCloudFolders, SearchSkipTempFiles, SearchSkipProgramParts),
                    hidden files, system files, and lists of folders, folder names, file types and regular
                    expressions. Fixed, the lists add to what people leave out themselves: they see your items with a
                    lock and can't remove them, but can still add their own.
  Results           which kinds of results show, in what order and how many of each (SearchResults: Apps, Settings,
                    Files, Web, one per line; a kind left out is never shown), and how they look.
  Search box        regular expression mode, and which parts of the search box and results show.
SearchSkipPatterns are regular expressions matched against the whole path, whatever the case. A file or folder that
matches is never found, nor anything in a folder that matches. For example:
    \\Payroll(\\|$)                    the Payroll folder and everything in it, wherever it is
    \.(key|pem|pfx)$                   key and certificate files
    ^C:\\Users\\[^\\]+\\Secret(\\|$)   a Secret folder at the top of anyone's home folder
A pattern that isn't valid is ignored. People see theirs marked in Settings.

What people can type in the search box: words (all of them have to be in the name, or name a folder it's in),
"exact text", wildcards (*.pdf, img_??.png), -word to leave out, ext:pdf,docx, kind:folder, document, picture, video,
music, archive, code or app, size:>10mb, size:<1k, size:1mb..1gb, modified:today, yesterday, week, month, year, 2025,
2025-03, >2026-01-01 or <2026-01-01 (date: the same), and regular expressions with re:pattern or /pattern/.

EVERY VALUE
===========
The same names work under Software\Policies\Startline (fixed) and under Software\Policies\Startline\Recommended
(defaults people can change). Values marked [fixed only] can't be defaults.

APPEARANCE
----------
Experience
    Look
    REG_SZ, one of: Windows11, Windows10, Windows7, WindowsXp, Startline
    Which look the Start menu, search and the settings window use.
Language
    Language
    REG_SZ, one of: system, en, de, it
    The language Startline shows its text in: system follows the language Windows is shown in when it's English, German or Italian, and uses English otherwise.
TextAndIconSize
    Text and icon size
    REG_DWORD, 80 to 150 (percent)
    In percent. The settings window offers 90 (small), 100 (normal) and 115 (large).
Mica
    Mica behind the Start menu
    REG_DWORD, 1 on, 0 off
    Windows 11's Mica material behind the Start menu, tinted by the desktop picture, instead of frosted glass. Windows 11 version 22H2 and later; older Windows ignores it. For the Windows 11 and Startline looks.
MicaLight
    Lighter Mica through the search box and the selected item
    REG_DWORD, 1 on, 0 off
    With Mica on, lets a lighter shade of it through the search box and the selected item instead of covering them.
ColorTheme
    Color theme
    REG_SZ, one of: Stone, Graphite, Moss, Harbor, Clay, Black, Daylight, Paper
    Only the Startline look uses this.
Color_Background
    Background color
    REG_SZ, #RRGGBB
    Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.
Color_Panels
    Panel color
    REG_SZ, #RRGGBB
    Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.
Color_Text
    Text color
    REG_SZ, #RRGGBB
    Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.
Color_QuietText
    Quiet text color
    REG_SZ, #RRGGBB
    Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.
Color_Accent
    Accent color
    REG_SZ, #RRGGBB
    Written as #RRGGBB. Replaces this color of the theme. Only the Startline look uses this.
Corners
    Window corners
    REG_SZ, one of: Round, Slight, Square
    Only the Startline look uses this.
FrostedGlass
    Frosted glass
    REG_DWORD, 1 on, 0 off
    Blurs what's behind the menu. Only the Startline look uses this.
Opacity
    How solid the menu is
    REG_DWORD, 50 to 100 (percent)
    In percent, from 50 (most see-through) to 100 (solid). Only the Startline look uses this.

START MENU
----------
Sections
    What the Start menu shows
    list: REG_DWORD 1, its items in a subkey of the same name
    One per line, in the order they should appear: Pinned, Recent, Frequent, AllApps. Any left out are hidden.
RecentCount
    Number of recent files
    REG_DWORD, 2 to 20
FrequentCount
    Number of most-used items
    REG_DWORD, 2 to 20
SearchAtBottom
    Search bar at the bottom
    REG_DWORD, 1 on, 0 off
    Only the Startline look uses this.
Columns
    Pinned apps per row
    REG_DWORD, 3 to 12
    Only the Startline look uses this.
TileSize
    Pinned app size
    REG_DWORD, 52 to 132
    Only the Startline look uses this.
ShowLabels
    Names under pinned apps
    REG_DWORD, 1 on, 0 off
    Only the Startline look uses this.
CompactRows
    Compact lists
    REG_DWORD, 1 on, 0 off
    Only the Startline look uses this.
HorizontalPosition
    Where it opens across the screen
    REG_SZ, one of: Left, Center, Right, FollowStart
    Only the Startline look uses this.
VerticalPosition
    Where it opens up and down the screen
    REG_SZ, one of: Top, Middle, Bottom
    Only the Startline look uses this.
Margin
    Gap from the screen edge
    REG_DWORD, 0 to 80
    Only the Startline look uses this.
Width
    Menu width
    REG_DWORD, 420 to 1200
    Only the Startline look uses this.
Height
    Menu height
    REG_DWORD, 420 to 1100
    Only the Startline look uses this.

PINNED APPS
-----------
PinnedApps
    Pinned apps
    list: REG_DWORD 1, its items in a subkey of the same name
    One per line: Section|Name|Target or Section|Name|Target|Arguments. Target is a file, folder, program or web address, or app:ID for an installed app. Environment variables such as %USERPROFILE% work. As a fixed setting this replaces the person's pins and stops them changing pins; as a default it's the pins they start with.

SEARCH: WHERE TO SEARCH
-----------------------
SearchFolders
    Folders to search
    list: REG_DWORD 1, its items in a subkey of the same name
    One folder or drive per line. Environment variables such as %USERPROFILE% work. This replaces the person's own places.
SearchNetworkDrives
    Search network drives
    REG_DWORD, 1 on, 0 off
    Includes any network drive as it gets attached. Big shares can take a while to read.
SearchUsbDrives
    Search USB drives
    REG_DWORD, 1 on, 0 off
    Includes USB sticks and disks as they get plugged in.

SEARCH: NEVER FOUND
-------------------
SearchSkipFolders
    Folders never found
    list: REG_DWORD 1, its items in a subkey of the same name; fixed, they're added to people's own
    One folder per line, such as %USERPROFILE%\AppData. Everything inside is never found either.
SearchSkipFolderNames
    Folder names never found
    list: REG_DWORD 1, its items in a subkey of the same name; fixed, they're added to people's own
    One name per line. A folder with this name is never found, nor anything in it, wherever it is.
SearchSkipFileTypes
    File types never found
    list: REG_DWORD 1, its items in a subkey of the same name; fixed, they're added to people's own
    One per line, such as .iso.
SearchSkipPatterns
    Paths never found (regular expressions)
    list: REG_DWORD 1, its items in a subkey of the same name; fixed, they're added to people's own
    One regular expression per line, matched against the whole path whatever the case. A file or folder that matches is never found, nor anything in a folder that matches. For example \\Payroll(\\|$) or \.(key|pem|pfx)$. A pattern that isn't valid is ignored.
SearchSkipHidden
    Hidden files and folders are never found
    REG_DWORD, 1 on, 0 off
    Things Windows keeps out of sight. Disabled finds them.
SearchSkipSystem
    System files are never found
    REG_DWORD, 1 on, 0 off
    Files and folders Windows marks as part of the system, such as desktop.ini. Disabled finds them.
SearchSkipAppData
    Never found: App data
    REG_DWORD, 1 on, 0 off
    Settings and caches programs keep in AppData. Enabled never finds them, disabled always finds them.
SearchSkipWindows
    Never found: Windows and installed programs
    REG_DWORD, 1 on, 0 off
    Windows, Program Files and other system folders. Enabled never finds them, disabled always finds them.
SearchSkipDeveloperFiles
    Never found: Developer clutter
    REG_DWORD, 1 on, 0 off
    node_modules, .git, build output and similar. Enabled never finds them, disabled always finds them.
SearchSkipCloudFolders
    Never found: Cloud folders
    REG_DWORD, 1 on, 0 off
    OneDrive, Dropbox and Google Drive folders. Enabled never finds them, disabled always finds them.
SearchSkipTempFiles
    Never found: Temporary and log files
    REG_DWORD, 1 on, 0 off
    Leftovers such as .tmp, .log and .bak. Enabled never finds them, disabled always finds them.
SearchSkipProgramParts
    Never found: Program parts
    REG_DWORD, 1 on, 0 off
    Files you never open yourself, such as .dll and .sys. Enabled never finds them, disabled always finds them.

SEARCH: RESULTS
---------------
SearchResults
    Kinds of results
    list: REG_DWORD 1, its items in a subkey of the same name
    One per line, first to last: Apps, Settings, Files, Web. A kind left out is never shown.
SearchMaxApps
    Most apps shown in the results
    REG_DWORD, 1 to 30
SearchMaxSettings
    Most settings shown in the results
    REG_DWORD, 1 to 30
SearchMaxFiles
    Most files shown in the results
    REG_DWORD, 5 to 200
SearchShowFolders
    Show the folder under each file
    REG_DWORD, 1 on, 0 off
SearchWebName
    Name of the web search
    REG_SZ
    Shown as: Search <name> for ...
SearchWebAddress
    Web search address
    REG_SZ
    Must start with http:// or https:// and hold {} where the words go. Anything else is ignored.
SearchChangeWindowsSettings
    Change Windows settings from search
    REG_DWORD, 1 on, 0 off
    Disabled removes every Windows setting search can change.
SearchWindowsSettingsPages
    Shortcuts to Windows settings and tools
    REG_DWORD, 1 on, 0 off
    Disabled removes search's shortcuts to Windows settings pages and tools, and the Control Panel, printer and default program links.
SearchWindowsSettings  [fixed only]
    Windows settings search can change
    REG_DWORD values named Allow_<item>, 1 allowed, 0 removed. Items: Setting_Wifi, Setting_Bluetooth, Setting_DarkMode, Setting_AppsDark, Setting_SystemDark, Setting_KeepAwake, Setting_Mute, Setting_MicMute, Setting_Transparency, Setting_AccentTaskbar, Setting_AccentTitleBars, Setting_AutoHideTaskbar, Setting_TaskbarAlign, Setting_TaskViewButton, Setting_ClockSeconds, Setting_FileExtensions, Setting_HiddenFiles, Setting_ItemCheckboxes, Setting_CompactExplorer, Setting_ClipboardHistory, Setting_GameMode, Setting_Animations, Setting_SnapWindows, Setting_DragContents, Setting_FontSmoothing, Setting_WindowShadows, Setting_HoverActivate, Setting_ScreenSaver, Setting_PointerPrecision, Setting_SwapMouseButtons, Setting_StickyKeys, Setting_Autostart, Setting_Volume, Setting_MicVolume, Setting_Brightness, Setting_MouseSpeed, Setting_ScrollLines, Setting_DoubleClick, Setting_KeyRepeatSpeed, Setting_KeyRepeatDelay, Setting_ScreenTimeout, Setting_SleepTimeout, Setting_PowerMode
    Unticked settings are removed from search.

SEARCH: SEARCH BOX
------------------
SearchRegex
    Treat searches as regular expressions
    REG_DWORD, 1 on, 0 off
    The whole search is one regular expression, matched against names whatever the case. The switch in the search box turns it on and off.
SearchShowRegexSwitch
    Regular expression switch in the search box
    REG_DWORD, 1 on, 0 off
    Disabled hides the switch, and searches are then never regular expressions unless SearchRegex is enabled.
SearchShowHelp
    Help button in the search box
    REG_DWORD, 1 on, 0 off
    The ? that shows what can be typed: words, "exact text", wildcards, -without, ext:, kind:, size:, modified: and regular expressions.
SearchFileTypeButtons
    File type buttons under search
    REG_DWORD, 1 on, 0 off
    The buttons that narrow results to one kind of file.
SearchShowTime
    Show how long a search took
    REG_DWORD, 1 on, 0 off
SearchShowHints
    Keyboard hints under the results
    REG_DWORD, 1 on, 0 off
    The line that says what Enter, Ctrl+Enter, Shift+Enter and Esc do.

GENERAL
-------
StartWithWindows
    Start with Windows
    REG_DWORD, 1 on, 0 off
    Keeps the Start menu working after a restart.
TakeOverStartKey
    Start key and button open this menu
    REG_DWORD, 1 on, 0 off
    Shift + Start still opens the Windows menu.
TakeOverSearchKey
    Start + S opens this menu too
    REG_DWORD, 1 on, 0 off
    Other Windows shortcuts keep working.
SkipWelcome
    Skip the welcome note
    REG_DWORD, 1 on, 0 off
    Enabled hides the note shown on the first run.

UPDATES
-------
AutomaticUpdates
    Install updates automatically
    REG_DWORD, 1 on, 0 off
    Startline installs new versions by itself, restarting while its menu is closed. On an install for the whole PC, the Startline Updater task installs them as SYSTEM within the hour. Disabled leaves updates to the person, or on an install for the whole PC, to the administrator.
CheckForUpdates  [fixed only]
    Look for updates
    REG_DWORD, 1 on, 0 off
    Disabled stops Startline looking for new versions at all, for organizations that deploy them their own way.

WHAT PEOPLE CAN DO
------------------
Commands  [fixed only]
    Commands people can use
    REG_DWORD values named Allow_<item>, 1 allowed, 0 removed. Items: Command_Lock, Command_Sleep, Command_SignOut, Command_SwitchUser, Command_Restart, Command_ShutDown, Command_EmptyRecycleBin, Command_Customize, Command_PinPage, Command_RealStart, Command_Quit, Command_SettingsFolder, Command_ResetSettings
    Unticked commands are removed everywhere they appear. Removing Customize Startline also closes off the settings window.
RightClickOptions  [fixed only]
    Right-click options
    REG_DWORD values named Allow_<item>, 1 allowed, 0 removed. Items: Menu_RunAsAdmin, Menu_OpenWith, Menu_OpenLocation, Menu_Copy, Menu_Uninstall, Menu_Recycle, Menu_Properties, Menu_ShellMenu
    Unticked options are removed. Their keyboard shortcuts stop working too.
