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
; ----------------------------- App Information -----------------------------
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

; ----------------------------- Directory Settings -----------------------------
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
AllowNoIcons=yes
DisableProgramGroupPage=yes

; ----------------------------- License & Info -----------------------------
LicenseFile=files\LICENSE
InfoBeforeFile=files\README.txt
; InfoAfterFile=files\CHANGELOG.txt   ; Uncomment if you have changelog

; ----------------------------- Output Settings -----------------------------
OutputDir=output
OutputBaseFilename=rustsp-{#MyAppVersion}-setup-x64
SetupIconFile=files\rustsp.ico
UninstallDisplayIcon={app}\bin\{#MyAppExeName}
UninstallDisplayName={#MyAppName} {#MyAppVersion}

; ----------------------------- Compression Settings -----------------------------
Compression=lzma2/ultra64
SolidCompression=yes
LZMAUseSeparateProcess=yes
LZMADictionarySize=65536
LZMANumFastBytes=273

; ----------------------------- Installer UI Settings -----------------------------
WizardStyle=modern
WizardSizePercent=120
WindowVisible=no
WindowShowCaption=yes
WindowResizable=yes
ShowLanguageDialog=auto
DisableWelcomePage=no

; ----------------------------- Privilege Settings -----------------------------
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

; ----------------------------- Uninstaller Settings -----------------------------
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
english.InstallVSCodeExtension=Install VS Code extension (if VS Code is installed)

[Types]
Name: "full"; Description: "Full installation"
Name: "compact"; Description: "Compact installation (binaries only)"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

[Components]
Name: "main"; Description: "RustS+ Compiler and Tools"; Types: full compact custom; Flags: fixed
Name: "examples"; Description: "Example .rss Files"; Types: full custom
Name: "vscode"; Description: "VS Code Extension"; Types: full custom

[Tasks]
Name: "addpath"; Description: "{cm:AddToPath}"; GroupDescription: "Environment:"; Flags: checkedonce
Name: "desktopicon"; Description: "{cm:CreateDesktopShortcut}"; GroupDescription: "Additional shortcuts:"
Name: "associaterss"; Description: "{cm:AssociateRssFiles}"; GroupDescription: "File associations:"

[Dirs]
Name: "{app}\bin"
Name: "{app}\lib"
Name: "{app}\examples"; Components: examples

[Files]
; Main binaries
Source: "files\rustsp.exe"; DestDir: "{app}\bin"; Flags: ignoreversion; Components: main
Source: "files\cargo-rustsp.exe"; DestDir: "{app}\bin"; Flags: ignoreversion; Components: main

; License and documentation
Source: "files\LICENSE"; DestDir: "{app}"; Flags: ignoreversion; Components: main
Source: "files\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme; Components: main

; Documentation (optional component)
Source: "files\examples\*"; DestDir: "{app}\examples"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: examples

; VS Code extension (optional)
Source: "files\vscode\rustsp-*.vsix"; DestDir: "{app}\vscode"; Flags: ignoreversion; Components: vscode; Check: FileExists('files\vscode\rustsp-*.vsix')

[Icons]
; Start Menu
Name: "{group}\RustS+ Documentation"; Filename: "{app}\README.md"; Components: main
Name: "{group}\RustS+ Examples"; Filename: "{app}\examples"; Components: examples
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"

; Desktop shortcut (optional)
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
; Post-install: Show README
Filename: "{app}\README.md"; Description: "View README documentation"; Flags: postinstall shellexec skipifsilent unchecked

; Post-install: Open examples folder
Filename: "{app}\examples"; Description: "Open examples folder"; Flags: postinstall shellexec skipifsilent unchecked; Components: examples

; Install VS Code extension if selected
Filename: "code"; Parameters: "--install-extension ""{app}\vscode\rustsp-*.vsix"""; \
    Description: "Install VS Code extension"; Flags: postinstall runhidden skipifsilent; \
    Components: vscode; Check: IsVSCodeInstalled

[UninstallRun]
; Cleanup VS Code extension on uninstall
Filename: "code"; Parameters: "--uninstall-extension rustsp.rustsp-lang"; Flags: runhidden; Check: IsVSCodeInstalled

[UninstallDelete]
Type: filesandordirs; Name: "{app}\bin"
Type: filesandordirs; Name: "{app}\lib"
Type: filesandordirs; Name: "{app}\examples"
Type: dirifempty; Name: "{app}"

[Code]
// ============================================================================
// Pascal Script Functions
// ============================================================================

var
  FinishedInstall: Boolean;

// ----------------------------------------------------------------------------
// Check if path needs to be added to PATH environment variable
// ----------------------------------------------------------------------------
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
  // Check if path already exists (case-insensitive)
  Result := Pos(';' + Lowercase(ExpandedParam) + ';', ';' + Lowercase(OrigPath) + ';') = 0;
end;

// ----------------------------------------------------------------------------
// Check if VS Code is installed
// ----------------------------------------------------------------------------
function IsVSCodeInstalled: Boolean;
var
  VSCodePath: string;
begin
  Result := RegQueryStringValue(HKEY_CURRENT_USER, 
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{771FD6B0-FA20-440A-A002-3B3BAC16DC50}_is1',
    'InstallLocation', VSCodePath) or
    RegQueryStringValue(HKEY_LOCAL_MACHINE,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{EA457B21-F73E-494C-ACAB-524FDE069978}_is1',
    'InstallLocation', VSCodePath) or
    FileExists(ExpandConstant('{localappdata}\Programs\Microsoft VS Code\Code.exe'));
end;

// ----------------------------------------------------------------------------
// Check if directory exists (for conditional file installation)
// ----------------------------------------------------------------------------
function DirExists(DirName: string): Boolean;
begin
  Result := DirExists(ExpandConstant('{src}\' + DirName));
end;

// ----------------------------------------------------------------------------
// Check if file pattern exists
// ----------------------------------------------------------------------------
function FileExists(FileName: string): Boolean;
var
  FindRec: TFindRec;
begin
  Result := FindFirst(ExpandConstant('{src}\' + FileName), FindRec);
  if Result then
    FindClose(FindRec);
end;

// ----------------------------------------------------------------------------
// Remove path from PATH on uninstall
// ----------------------------------------------------------------------------
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
    
  // Adjust P to account for the leading semicolon we added
  P := P - 1;
  
  // Remove the path (including leading or trailing semicolon)
  if P = 0 then
    Delete(Paths, 1, Length(Path) + 1)  // At start, remove path + trailing semicolon
  else
    Delete(Paths, P, Length(Path) + 1); // In middle/end, remove semicolon + path
    
  RegWriteStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Paths);
end;

// ----------------------------------------------------------------------------
// Refresh environment variables after PATH change
// ----------------------------------------------------------------------------
procedure RefreshEnvironment;
var
  S: AnsiString;
begin
  S := 'Environment';
  // Broadcast WM_SETTINGCHANGE to notify other applications
  // Note: This requires SendBroadcastMessage which isn't directly available,
  // but the installer handles this automatically on completion
end;

// ----------------------------------------------------------------------------
// Initialize Setup
// ----------------------------------------------------------------------------
function InitializeSetup(): Boolean;
begin
  Result := True;
  FinishedInstall := False;
end;

// ----------------------------------------------------------------------------
// Called when installation is complete
// ----------------------------------------------------------------------------
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    FinishedInstall := True;
    // Notify user that they may need to restart terminal
    if IsTaskSelected('addpath') then
    begin
      MsgBox('RustS+ has been added to your PATH.' + #13#10 + #13#10 +
             'Please restart any open terminals or command prompts ' +
             'to use the rustsp command.', mbInformation, MB_OK);
    end;
  end;
end;

// ----------------------------------------------------------------------------
// Uninstall: Remove from PATH
// ----------------------------------------------------------------------------
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    RemovePath(ExpandConstant('{app}\bin'));
  end;
end;

// ----------------------------------------------------------------------------
// Custom wizard page for showing system requirements (optional)
// ----------------------------------------------------------------------------
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
