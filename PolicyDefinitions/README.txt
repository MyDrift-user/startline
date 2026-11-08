Startline Group Policy templates
================================

Copy Startline.admx to C:\Windows\PolicyDefinitions and en-US\Startline.adml to
C:\Windows\PolicyDefinitions\en-US (or the same places in your domain's Central Store).
The settings appear under Administrative Templates > Startline, for computers and for users.
Where both set the same policy, the computer's value wins.

The values are stored in Software\Policies\Startline. Startline picks up changes within a minute
while it's open. Removing a policy brings back the person's own choice.

These two files are written by the app from its list of policies. To write them again:
    startline.exe --write-policy-templates <folder>

SEARCH
The search policies are under Startline > Search, in three parts:
  Where to search   the places, and whether network and USB drives are searched as they attach.
  Never found       what search never shows: six ready-made groups (SearchSkipAppData, SearchSkipWindows,
                    SearchSkipDeveloperFiles, SearchSkipCloudFolders, SearchSkipTempFiles,
                    SearchSkipProgramParts), hidden files, system files, and lists of folders, folder names,
                    file types and regular expressions. The lists add to what people leave out themselves:
                    they see your items with a lock and can't remove them, but can still add their own.
  Results           which kinds of results show and in what order (SearchResults: Apps, Settings, Files,
                    Web, one per line; a kind left out is never shown), and how they look.
SearchSkipPatterns are regular expressions matched against the whole path, whatever the case. A file or
folder that matches is never found, nor anything in a folder that matches. For example:
    \\Payroll(\\|$)                 the Payroll folder and everything in it, wherever it is
    \.(key|pem|pfx)$                key and certificate files
    ^C:\\Users\\[^\\]+\\Secret(\\|$)   a Secret folder at the top of anyone's home folder
A pattern that isn't valid is ignored. People see theirs marked in Settings.

REGISTRY
For Group Policy Preferences or scripts, write the values to Software\Policies\Startline under HKLM or
HKCU. Switches and numbers are REG_DWORD, text is REG_SZ. A list is a REG_DWORD of 1 under the list's
name, with its items as REG_SZ values named 1, 2, 3 and so on in a subkey of the same name, for example
    Software\Policies\Startline              SearchSkipFolderNames = 1
    Software\Policies\Startline\SearchSkipFolderNames   1 = Payroll, 2 = HR
Tick boxes such as SearchWindowsSettings are REG_DWORD values named Allow_<item>, 0 or 1.
