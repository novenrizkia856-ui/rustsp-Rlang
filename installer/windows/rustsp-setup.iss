; ============================================================================
; RustS+ Installer Script
; The Programming Language with Effect Honesty
; Part of DSDN (Data Semi-Decentral Network) Project
; ============================================================================
; Build: iscc rustsp-setup.iss
; Requirements: Inno Setup 6.2+
; ============================================================================

#define MyAppName "RustS+"
#define MyAppVersion "1.0.0"
#define MyAppPublisher "DSDN Project"
#define MyAppURL "https://github.com/novenrizkia856-ui/rustsp-Rlang"
#define MyAppExeName "rustsp.exe"
#define MyAppDescription "The Programming Language with Effect Honesty"
#define MyAppCopyright "Copyright (c) 2026 RustS+ Contributors"

[Setup]
AppId={{A7D8E3F2-4B6C-9D1E-2F3A-5B7C8D9E0F1A}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
AppCopyright={#MyAppCopyright}
VersionInfoVersion={#MyAppVersion}
VersionInfoDescription={#MyAppDescription}

DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
AllowNoIcons=yes
DisableProgramGroupPage=yes

LicenseFile=files\LICENSE
InfoBeforeFile=files\README.txt

OutputDir=output
OutputBaseFilename=rustsp-{#MyAppVersion}-setup-x64
SetupIconFile=files\rustsp.ico
UninstallDisplayIcon={app}\bin\{#MyAppExeName}
UninstallDisplayName={#MyAppName} {#MyAppVersion}

Compression=lzma2/ultra64
SolidCompression=yes
LZMAUseSeparateProcess=yes
LZMADictionarySize=65536
LZMANumFastBytes=273

WizardStyle=modern
WizardSizePercent=120
WindowResizable=yes
DisableWelcomePage=no

PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

Uninstallable=yes
UninstallFilesDir={app}\uninstall
CreateUninstallRegKey=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
BeveledLabel=RustS+ - Where Logic Safety Meets Memory Safety

[CustomMessages]
english.AddToPath=Add {#MyAppName} to system PATH (recommended)
english.CreateDesktopShortcut=Create desktop shortcut
english.AssociateRssFiles=Associate .rss files with {#MyAppName}

[Types]
Name: "full"; Description: "Full installation"
Name: "compact"; Description: "Compact installation (binaries only)"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

[Components]
Name: "main"; Description: "RustS+ Compiler and Tools"; Types: full compact custom; Flags: fixed
Name: "examples"; Description: "Example .rss Files"; Types: full custom

[Tasks]
Name: "addpath"; Description: "{cm:AddToPath}"; GroupDescription: "Environment:"; Flags: checkedonce
Name: "desktopicon"; Description: "{cm:CreateDesktopShortcut}"; GroupDescription: "Additional shortcuts:"
Name: "associaterss"; Description: "{cm:AssociateRssFiles}"; GroupDescription: "File associations:"

[Dirs]
Name: "{app}\bin"
Name: "{app}\examples"; Components: examples

[Files]
; Main binaries
Source: "files\rustsp.exe"; DestDir: "{app}\bin"; Flags: ignoreversion; Components: main
Source: "files\cargo-rustsp.exe"; DestDir: "{app}\bin"; Flags: ignoreversion; Components: main

; License
Source: "files\LICENSE"; DestDir: "{app}"; Flags: ignoreversion; Components: main

; Examples (optional)
Source: "files\examples\*"; DestDir: "{app}\examples"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: examples

[Icons]
Name: "{group}\RustS+ Examples"; Filename: "{app}\examples"; Components: examples
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\RustS+ Terminal"; Filename: "{cmd}"; Parameters: "/k cd /d ""{app}"" && echo RustS+ {#MyAppVersion} - Type 'rustsp --help' for usage"; Tasks: desktopicon; IconFilename: "{app}\bin\{#MyAppExeName}"

[Registry]
; Add to PATH (user level)
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; \
    ValueData: "{olddata};{app}\bin"; Tasks: addpath; Check: NeedsAddPath('{app}\bin')

; File association for .rss files
Root: HKCU; Subkey: "Software\Classes\.rss"; ValueType: string; ValueData: "RustSPlus.SourceFile"; Tasks: associaterss; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\RustSPlus.SourceFile"; ValueType: string; ValueData: "RustS+ Source File"; Tasks: associaterss; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\RustSPlus.SourceFile\DefaultIcon"; ValueType: string; ValueData: "{app}\bin\{#MyAppExeName},0"; Tasks: associaterss
Root: HKCU; Subkey: "Software\Classes\RustSPlus.SourceFile\shell\open\command"; ValueType: string; ValueData: """{app}\bin\{#MyAppExeName}"" ""%1"""; Tasks: associaterss

; Uninstall info
Root: HKCU; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}"; ValueType: string; ValueName: "InstallPath"; ValueData: "{app}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}"; ValueType: string; ValueName: "Version"; ValueData: "{#MyAppVersion}"

[Run]
Filename: "{app}\examples"; Description: "Open examples folder"; Flags: postinstall shellexec skipifsilent unchecked; Components: examples

[UninstallDelete]
Type: filesandordirs; Name: "{app}\bin"
Type: filesandordirs; Name: "{app}\examples"
Type: dirifempty; Name: "{app}"

[Code]
function NeedsAddPath(Param: string): Boolean;
var
  OrigPath: string;
  ExpandedParam: string;
begin
  ExpandedParam := ExpandConstant(Param);
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
  begin
    Result := True;
    Exit;
  end;
  Result := Pos(';' + Lowercase(ExpandedParam) + ';', ';' + Lowercase(OrigPath) + ';') = 0;
end;

procedure RemovePath(Path: string);
var
  Paths: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Paths) then
    Exit;
  
  P := Pos(';' + Lowercase(Path) + ';', ';' + Lowercase(Paths) + ';');
  if P = 0 then
    Exit;
    
  P := P - 1;
  
  if P = 0 then
    Delete(Paths, 1, Length(Path) + 1)
  else
    Delete(Paths, P, Length(Path) + 1);
    
  RegWriteStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    if IsTaskSelected('addpath') then
    begin
      MsgBox('RustS+ has been added to your PATH.' + #13#10 + #13#10 +
             'Please restart any open terminals or command prompts ' +
             'to use the rustsp command.', mbInformation, MB_OK);
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    RemovePath(ExpandConstant('{app}\bin'));
  end;
end;

procedure InitializeWizard;
var
  InfoPage: TOutputMsgMemoWizardPage;
begin
  InfoPage := CreateOutputMsgMemoPage(wpWelcome,
    'System Requirements', 
    'Please review the following requirements before continuing.',
    'RustS+ requires the following:',
    'System Requirements:' + #13#10 +
    '- Windows 10/11 (64-bit)' + #13#10 +
    '- Rust toolchain (rustc, cargo) installed' + #13#10 +
    '- 50 MB free disk space' + #13#10 + #13#10 +
    'Recommended:' + #13#10 +
    '- Visual Studio Code with Rust extension' + #13#10 +
    '- Git for version control' + #13#10 + #13#10 +
    'RustS+ compiles your .rss files to standard Rust, ' + #13#10 +
    'then uses rustc to produce native executables.');
end;
