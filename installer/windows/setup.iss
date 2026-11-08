; Built by installer\windows\package.ps1: ISCC /DAppVersion=x.y.z /DOutputName=name installer\windows\setup.iss
[Setup]
AppId={{4E1D8B7A-2C55-4F3B-A9D6-7B0E3C915F28}
AppName=Startline
AppVersion={#AppVersion}
AppPublisher=MyDrift
AppPublisherURL=https://github.com/MyDrift-user/startline
AppSupportURL=https://github.com/MyDrift-user/startline/issues
DefaultDirName={autopf}\Startline
DefaultGroupName=Startline
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename={#OutputName}
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\startline.exe
UninstallDisplayName=Startline
LicenseFile=..\..\LICENSE
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern

[Files]
Source: "..\..\target\release\startline.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Startline"; Filename: "{app}\startline.exe"

[Run]
Filename: "{app}\startline.exe"; Description: "{cm:LaunchProgram,Startline}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Startline stays running in the background, and its Start with Windows switch would point at a deleted program.
Filename: "{sys}\taskkill.exe"; Parameters: "/f /im startline.exe"; Flags: runhidden; RunOnceId: "CloseStartline"
Filename: "{sys}\reg.exe"; Parameters: "delete ""HKCU\Software\Microsoft\Windows\CurrentVersion\Run"" /v Startline /f"; Flags: runhidden; RunOnceId: "DeleteAutostart"

[Code]
// Startline holds its own file while it runs, so close it before installing over it.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/f /im startline.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;
