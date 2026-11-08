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
Source: "machine-install.txt"; DestDir: "{app}"; Flags: ignoreversion; Check: IsAdminInstallMode

[Icons]
Name: "{autoprograms}\Startline"; Filename: "{app}\startline.exe"

[Run]
Filename: "{app}\startline.exe"; Description: "{cm:LaunchProgram,Startline}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Startline stays running in the background, and its Start with Windows switch would point at a deleted program.
Filename: "{sys}\taskkill.exe"; Parameters: "/f /im startline.exe"; Flags: runhidden; RunOnceId: "CloseStartline"
Filename: "{sys}\reg.exe"; Parameters: "delete ""HKCU\Software\Microsoft\Windows\CurrentVersion\Run"" /v Startline /f"; Flags: runhidden; RunOnceId: "DeleteAutostart"
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /F /TN ""Startline Updater"""; Flags: runhidden; Check: IsAdminInstallMode; RunOnceId: "DeleteUpdateTask"

[UninstallDelete]
Type: filesandordirs; Name: "{app}\updates"

[Code]
// For a PC-wide install: a task installs updates as SYSTEM, so nobody gets an admin prompt. See src/update.rs.
// It runs this folder's program as SYSTEM, so only admins may change it, wherever it was installed.
// There's no task if that can't be done. Only on the folder, as files inherit from it: with /T the same command
// leaves files with no permissions.
procedure CurStepChanged(CurStep: TSetupStep);
var
  Code: Integer;
begin
  if (CurStep = ssPostInstall) and IsAdminInstallMode then
  begin
    if Exec(ExpandConstant('{sys}\icacls.exe'), '"' + ExpandConstant('{app}') + '" /inheritance:r /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F *S-1-5-32-545:(OI)(CI)RX /C', '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0) then
      Exec(ExpandConstant('{sys}\schtasks.exe'), '/Create /F /TN "Startline Updater" /SC HOURLY /RU SYSTEM /RL HIGHEST /TR "\"' + ExpandConstant('{app}') + '\startline.exe\" --update-service"', '', SW_HIDE, ewWaitUntilTerminated, Code)
    else
      Log('Could not lock the install folder, so there is no update task');
  end;
end;

// Startline holds its own file while it runs, so close it before installing over it.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/f /im startline.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;
