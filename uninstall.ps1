# Removes Deck: its scheduled tasks, shortcuts (Start menu, desktop profile shortcuts) and
# C:\Program Files\Deck. Settings in %APPDATA%\Deck are kept unless -Purge is given.
param([switch]$Purge)
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    $a = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`"" + $(if ($Purge) { " -Purge" } else { "" })
    Start-Process powershell -Verb RunAs -ArgumentList $a
    exit
}

# quitting Deck hands the fans back to the BIOS, the GPU fan to the driver and clears the overlay
$exe = "$env:ProgramFiles\Deck\Deck.exe"
if ((Get-Process Deck -ErrorAction SilentlyContinue) -and (Test-Path $exe)) {
    Start-Process $exe -ArgumentList "--quit"
    for ($i = 0; $i -lt 20 -and (Get-Process Deck -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
}
Get-Process Deck -ErrorAction SilentlyContinue | Stop-Process -Force
if (Test-Path "$env:ProgramFiles\Deck\Deck.exe") { Start-Process "$env:ProgramFiles\Deck\Deck.exe" -ArgumentList "--release-fans" -Wait -ErrorAction SilentlyContinue }
# Claude Sync's hooks out of Claude Code's settings (and the hook program), plus Deck's firewall rules
if (Test-Path $exe) { Start-Process $exe -ArgumentList "--uninstall-cleanup" -Wait -ErrorAction SilentlyContinue }
foreach ($r in "Deck phone remote", "Deck between your PCs") { netsh advfirewall firewall delete rule name="$r" | Out-Null }

# every task in the \Deck\ folder
$svc = New-Object -ComObject Schedule.Service
$svc.Connect()
try {
    $f = $svc.GetFolder("\Deck")
    foreach ($t in @($f.GetTasks(1))) { $f.DeleteTask($t.Name, 0) }
    $svc.GetFolder("\").DeleteFolder("Deck", 0)
} catch {}

# shortcuts: the Start menu entry and every profile shortcut (they run run.vbs "\Deck\Profile ...")
$sh = New-Object -ComObject WScript.Shell
$places = @("$env:APPDATA\Microsoft\Windows\Start Menu\Programs", [Environment]::GetFolderPath("Desktop"))
foreach ($p in $places) {
    Get-ChildItem $p -Filter *.lnk -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
        $l = $sh.CreateShortcut($_.FullName)
        if ($l.Arguments -match '"\\Deck\\(Open|Profile [^"]+)"' -or $l.TargetPath -match '\\Deck\\Deck\.exe$') { Remove-Item $_.FullName -Force }
    }
}
Remove-Item "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Deck" -Recurse -Force -ErrorAction SilentlyContinue

# the PawnIO driver (fans, sensors): only when Deck installed it, other tools may share it otherwise
if (Test-Path "$env:ProgramData\Deck\pawnio\installed-by-deck") {
    $dev = pnputil /enum-devices /class SoftwareDevice | Out-String
    foreach ($b in ($dev -replace "`r", "") -split "`n`n") {
        if ($b -match 'ROOT\\PAWNIO\\' -and $b -match 'Instance ID:\s*(\S+)') { pnputil /remove-device $Matches[1] | Out-Null }
    }
    $pub = $null
    foreach ($l in (pnputil /enum-drivers)) {
        if ($l -match '^\s*Published Name:\s*(\S+)') { $pub = $Matches[1] }
        elseif ($l -match '^\s*Original Name:\s*pawnio\.inf' -and $pub) { pnputil /delete-driver $pub /uninstall /force | Out-Null }
    }
}
sc.exe stop DeckPawnIO 2>$null | Out-Null
sc.exe delete DeckPawnIO 2>$null | Out-Null
Remove-Item "$env:ProgramData\Deck" -Recurse -Force -ErrorAction SilentlyContinue

Remove-Item "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Deck" -Recurse -Force -ErrorAction SilentlyContinue
Set-Location $env:TEMP
Remove-Item "$env:ProgramFiles\Deck" -Recurse -Force -ErrorAction SilentlyContinue
if ($Purge) { Remove-Item "$env:APPDATA\Deck" -Recurse -Force -ErrorAction SilentlyContinue }
Write-Host "Deck removed." -ForegroundColor Green
Start-Sleep 2
