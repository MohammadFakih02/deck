# Elevated (UAC is off on this PC, so Start-Process -Verb RunAs is silent; add -WindowStyle Hidden so no console shows): quit Deck, install the built exe, start it with the CDP debug port 9222. Log: %TEMP%\deck-install.log
$log = "$env:TEMP\deck-install.log"
Start-Transcript $log -Force | Out-Null
try {
    $exe = "$env:ProgramFiles\Deck\Deck.exe"
    if (Get-Process Deck -ErrorAction SilentlyContinue) {
        $t0 = Get-Date
        Start-Process $exe -ArgumentList "--quit"
        for ($i = 0; $i -lt 30 -and (Get-Process Deck -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
        Write-Host ("QUIT took {0:N1} s{1}" -f ((Get-Date) - $t0).TotalSeconds, $(if (Get-Process Deck -ErrorAction SilentlyContinue) { " (still running: forced)" } else { "" }))
    }
    Get-Process Deck -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep 1
    if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = [Environment]::GetEnvironmentVariable('CARGO_TARGET_DIR', 'User') }  # elevated shells don't inherit it
    & (Join-Path $PSScriptRoot '..\..\install.ps1') -NoBuild
    Write-Host "EXIT $LASTEXITCODE"
    Start-Sleep 3
    if (Get-Process Deck -ErrorAction SilentlyContinue) {
        Start-Process $exe -ArgumentList "--quit"
        for ($i = 0; $i -lt 30 -and (Get-Process Deck -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
        Get-Process Deck -ErrorAction SilentlyContinue | Stop-Process -Force
    }
    # the debug port goes on Deck's command line: an elevated Deck ignores WEBVIEW2_* variables
    Start-Process $exe -ArgumentList "--cdp-port=9222"
    Write-Host "STARTED debug"
} catch { Write-Host "ERROR $_" }
Stop-Transcript | Out-Null
