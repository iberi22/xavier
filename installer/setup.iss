; Inno Setup Script for Xavier
#define MyAppName "Xavier"
#define MyAppVersion "0.12.0"
#define MyAppPublisher "SouthWest AI Labs"
#define MyAppURL "https://github.com/iberi22/xavier"
#define MyAppExeName "xavier-panel.exe"

[Setup]
; NOTE: The value of AppId uniquely identifies this application. Do not use the same AppId value in installers for other applications.
; (To generate a new GUID, click Tools | Generate GUID inside the IDE.)
AppId={{e6e6e6e6-e6e6-e6e6-e6e6-e6e6e6e6e6e6}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
;AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppPublisher}\{#MyAppName}
DisableProgramGroupPage=yes
; Uncomment the following line to run in non administrative install mode (install for current user only.)
;PrivilegesRequired=lowest
OutputBaseFilename=XavierSetup
Compression=lzma
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "envpath"; Description: "Add Xavier to system PATH"; GroupDescription: "Additional tasks:"

[Files]
; Panel UI (Tauri App) - Main GUI application with system tray
; Note: The Tauri build outputs to target/release/app.exe
Source: "..\target\release\app.exe"; DestDir: "{app}"; DestName: "xavier-panel.exe"; Flags: ignoreversion
; Backend server binary (standalone)
Source: "..\target\release\xavier.exe"; DestDir: "{app}"; DestName: "xavier-server.exe"; Flags: ignoreversion
; TUI Dashboard binary
Source: "..\target\release\xavier-tui.exe"; DestDir: "{app}"; Flags: ignoreversion
; Configuration file
Source: "..\config\xavier.config.json"; DestDir: "{app}"; Flags: ignoreversion
; NOTE: Don't use "Flags: ignoreversion" on any shared system files

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Comment: "Xavier Memory Runtime (Panel UI)"
Name: "{autoprograms}\{#MyAppName} TUI Dashboard"; Filename: "{app}\xavier-tui.exe"; Comment: "Xavier Terminal Dashboard"
Name: "{autoprograms}\{#MyAppName} Server"; Filename: "{app}\xavier-server.exe"; Parameters: "http 8006"; Comment: "Xavier Backend Server (CLI)"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon; Comment: "Xavier Memory Runtime"
Name: "{userstartup}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Comment: "Start Xavier on login"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipfsreqterminate

[Code]
const
  SM_PATH = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

procedure CurStepChanged(CurStep: TSetupStep);
var
  Path: string;
begin
  if (CurStep = ssPostInstall) and IsTaskSelected('envpath') then
  begin
    if RegQueryStringValue(HKEY_LOCAL_MACHINE, SM_PATH, 'Path', Path) then
    begin
      if Pos(ExpandConstant('{app}'), Path) = 0 then
      begin
        Path := Path + ';' + ExpandConstant('{app}');
        if RegWriteStringValue(HKEY_LOCAL_MACHINE, SM_PATH, 'Path', Path) then
        begin
          Log('Successfully added to PATH');
        end
        else
        begin
          Log('Failed to add to PATH');
        end;
      end;
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Path: string;
  AppPath: string;
  P: Integer;
begin
  if CurUninstallStep = usUninstall then
  begin
    if RegQueryStringValue(HKEY_LOCAL_MACHINE, SM_PATH, 'Path', Path) then
    begin
      AppPath := ExpandConstant('{app}');
      P := Pos(';' + AppPath, Path);
      if P > 0 then
      begin
        Delete(Path, P, Length(';' + AppPath));
        RegWriteStringValue(HKEY_LOCAL_MACHINE, SM_PATH, 'Path', Path);
      end
      else
      begin
        P := Pos(AppPath + ';', Path);
        if P > 0 then
        begin
          Delete(Path, P, Length(AppPath + ';'));
          RegWriteStringValue(HKEY_LOCAL_MACHINE, SM_PATH, 'Path', Path);
        end;
      end;
    end;
  end;
end;
