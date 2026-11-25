param(
    [string]$ExePath = "",
    [switch]$NoDesktop,
    [switch]$NoStartMenu
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

if (-not $ExePath) {
    $candidates = @(
        (Join-Path $root "src-tauri\target\release\LimitKill.exe"),
        (Join-Path $root "src-tauri\target\release\limitkill.exe"),
        (Join-Path $root "src-tauri\target\debug\limitkill.exe")
    )
    $ExePath = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}

if (-not $ExePath -or -not (Test-Path $ExePath)) {
    throw "LimitKill.exe was not found. Build the app (npm run tauri build) or pass -ExePath."
}

$ExePath = (Resolve-Path $ExePath).Path
$exeDir = Split-Path -Parent $ExePath
$icon = Join-Path $root "src-tauri\icons\icon.ico"
$iconLocation = if (Test-Path $icon) { "$icon,0" } else { "$ExePath,0" }

$shell = New-Object -ComObject WScript.Shell
$targets = @()
if (-not $NoDesktop) {
    $targets += Join-Path ([Environment]::GetFolderPath("Desktop")) "LimitKill.lnk"
}
if (-not $NoStartMenu) {
    $startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
    if (-not (Test-Path $startMenu)) {
        New-Item -ItemType Directory -Path $startMenu -Force | Out-Null
    }
    $targets += Join-Path $startMenu "LimitKill.lnk"
}

foreach ($target in $targets) {
    $shortcut = $shell.CreateShortcut($target)
    $shortcut.TargetPath = $ExePath
    $shortcut.WorkingDirectory = $exeDir
    $shortcut.IconLocation = $iconLocation
    $shortcut.Description = "LimitKill - local AI coding workspace"
    $shortcut.Save()
    Write-Host "Created shortcut: $target"
}

Write-Host "Done. Target: $ExePath"
