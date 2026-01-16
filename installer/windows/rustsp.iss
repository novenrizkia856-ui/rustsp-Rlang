[Setup]
AppName=RustS+
AppVersion=1.0.0
DefaultDirName=C:\RustS+
DefaultGroupName=RustS+
UninstallDisplayIcon={app}\bin\rustsp.exe
OutputDir=.
OutputBaseFilename=rustsp-setup
Compression=lzma
SolidCompression=yes

[Files]
Source: "files\rustsp.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "files\cargo-rustsp.exe"; DestDir: "{app}\bin"; Flags: ignoreversion

[Tasks]
Name: "addpath"; Description: "Add RustS+ to PATH"; Flags: checkedonce

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; \
    ValueData: "{olddata};{app}\bin"; Tasks: addpath; Check: NeedsAddPath

[Code]
function NeedsAddPath(): Boolean;
var
  Paths: string;
begin
  if RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Paths) then
    Result := Pos('{app}\bin', Paths) = 0
  else
    Result := True;
end;
