# Builds Deck and installs it to C:\Program Files\Deck (asks for admin once).
# Deck then runs with admin rights through a logon scheduled task, so it never shows a UAC prompt:
#   \Deck\Deck  at logon (Start with Windows)      \Deck\Open  started by the Start menu shortcut
# Settings in %APPDATA%\Deck are kept.
param([switch]$NoBuild)
$ErrorActionPreference = "Stop"

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "$PSScriptRoot\src-tauri\target" }

if (-not $NoBuild) {
    # Tauri caches compressed copies of ui/ and never rewrites them; a build cut
    # short (e.g. a PC crash) leaves broken copies and the window says "asset not found".
    Remove-Item "$target\release\build\deck-*\out\tauri-codegen-assets" -Recurse -Force -ErrorAction SilentlyContinue
    Push-Location "$PSScriptRoot\src-tauri"
    try { cargo build --release; if ($LASTEXITCODE) { throw "build failed" } } finally { Pop-Location }
}

if (-not $admin) {
    # the copy step needs admin: run this script again elevated, without rebuilding
    $argList = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -NoBuild"
    if ($env:CARGO_TARGET_DIR) { $argList = "-NoProfile -ExecutionPolicy Bypass -Command `"`$env:CARGO_TARGET_DIR='$env:CARGO_TARGET_DIR'; & '$PSCommandPath' -NoBuild`"" }
    $p = Start-Process powershell -Verb RunAs -ArgumentList $argList -Wait -PassThru
    exit $p.ExitCode
}

function Fail($msg) {
    # the elevated window would close at once: keep the reason on screen
    Write-Host $msg -ForegroundColor Red
    Start-Sleep 20
    exit 1
}

$dir = Join-Path $env:ProgramFiles "Deck"
$built = "$target\release\deck.exe"
if (-not (Test-Path $built)) { Fail "No build at $built. Run this without -NoBuild." }

# a running Deck (and its watchdog, which would start it again) locks Deck.exe: an install that
# only warned and stopped here changed nothing
if (Get-Process Deck -ErrorAction SilentlyContinue) {
    Write-Host "Closing Deck..."
    if (Test-Path "$dir\Deck.exe") { Start-Process "$dir\Deck.exe" -ArgumentList "--quit" -ErrorAction SilentlyContinue }
    for ($i = 0; $i -lt 30 -and (Get-Process Deck -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
    Get-Process Deck -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep 1
}

New-Item -ItemType Directory -Force $dir | Out-Null
$want = (Get-FileHash $built).Hash
for ($i = 0; ; $i++) {
    try { Copy-Item $built "$dir\Deck.exe" -Force -ErrorAction Stop } catch {}
    if ((Get-FileHash "$dir\Deck.exe" -ErrorAction SilentlyContinue).Hash -eq $want) { break }
    if ($i -ge 10) { Fail "Deck.exe in $dir could not be replaced (still in use?). Nothing was installed." }
    Get-Process Deck -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep 1
}
Write-Host "Deck.exe replaced and checked: it matches the build."
Copy-Item "$PSScriptRoot\src-tauri\run.vbs" "$dir\run.vbs" -Force
if (Test-Path "$PSScriptRoot\src-tauri\icons\profiles") {
    New-Item -ItemType Directory -Force "$dir\icons" | Out-Null
    Copy-Item "$PSScriptRoot\src-tauri\icons\profiles\*.ico" "$dir\icons" -Force
}

# old per-user installs (v0.1.x): their exe, the Run key autostart and the NSIS uninstall entry
foreach ($old in "$env:LOCALAPPDATA\Deck", "$env:LOCALAPPDATA\Programs\Deck") {
    if (Test-Path "$old\deck.exe") { Remove-Item $old -Recurse -Force -ErrorAction SilentlyContinue }
}
Remove-ItemProperty HKCU:\Software\Microsoft\Windows\CurrentVersion\Run -Name Deck -ErrorAction SilentlyContinue
Remove-ItemProperty HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run -Name Deck -ErrorAction SilentlyContinue
Get-ChildItem HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall -ErrorAction SilentlyContinue |
    Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -eq "Deck" } | Remove-Item -Recurse -Force

# scheduled tasks (Deck.exe creates them so the XML lives in one place)
$p = Start-Process "$dir\Deck.exe" -ArgumentList "--setup-task" -Wait -PassThru
if ($p.ExitCode) { throw "Deck couldn't create its scheduled tasks." }

# uninstall entry (Apps & features)
$u = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Deck"
New-Item $u -Force | Out-Null
Set-ItemProperty $u DisplayName "Deck"
Set-ItemProperty $u DisplayIcon "$dir\Deck.exe"
Set-ItemProperty $u Publisher "moudi"
Set-ItemProperty $u InstallLocation $dir
Set-ItemProperty $u DisplayVersion ((Get-Item "$dir\Deck.exe").VersionInfo.ProductVersion)
Copy-Item "$PSScriptRoot\uninstall.ps1" "$dir\uninstall.ps1" -Force
Set-ItemProperty $u UninstallString "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$dir\uninstall.ps1`""
Set-ItemProperty $u NoModify 1
Set-ItemProperty $u NoRepair 1

# Start menu: opens Deck through the \Deck\Open task (admin, no UAC prompt)
$sh = New-Object -ComObject WScript.Shell
$lnk = $sh.CreateShortcut("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Deck.lnk")
$lnk.TargetPath = "$env:SystemRoot\System32\wscript.exe"
$lnk.Arguments = "`"$dir\run.vbs`" `"\Deck\Open`""
$lnk.IconLocation = "$dir\Deck.exe,0"
$lnk.WorkingDirectory = $dir
$lnk.Description = "Dashboard, launcher, hardware control and Windows power tools"
$lnk.Save()

# prove that apps Deck opens start as the normal user, not as admin
Remove-Item "$env:TEMP\deck-launch-test.result" -ErrorAction SilentlyContinue
Start-Process "$dir\Deck.exe" -ArgumentList "--launch-test" -Wait
$res = Get-Content "$env:TEMP\deck-launch-test.result" -ErrorAction SilentlyContinue
Write-Host "Launch test: $res"

schtasks /run /tn "\Deck\Open" | Out-Null
Write-Host "Installed to $dir and started." -ForegroundColor Green
Start-Sleep 3
