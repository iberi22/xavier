; Inno Setup Script for Xavier
#define MyAppName "Xavier"
#define MyAppVersion "0.6.1-beta"
#define MyAppPublisher "SouthWest AI Labs"
#define MyAppURL "https://github.com/iberi22/xavier"
#define MyAppExeName "xavier-gui.exe"

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
Source: "..\target\release\xavier.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\xavier-tui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\xavier-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\config\xavier.config.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\panel-ui\build\*"; DestDir: "{app}\panel-ui"; Flags: ignoreversion recursesubdirs createallsubdirs
; NOTE: Don't use "Flags: ignoreversion" on any shared system files

[Icons]
Name: "{autoprograms}\{#MyAppName} GUI"; Filename: "{app}\{#MyAppExeName}"
Name: "{autoprograms}\{#MyAppName} TUI"; Filename: "{app}\xavier-tui.exe"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

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
