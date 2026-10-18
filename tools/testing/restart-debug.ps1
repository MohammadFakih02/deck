Start-Process "$env:ProgramFiles\Deck\Deck.exe" -ArgumentList "--quit"
for ($i = 0; $i -lt 40 -and (Get-Process Deck -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
# the debug port goes on Deck's command line: an elevated Deck ignores WEBVIEW2_* variables
Start-Process "$env:ProgramFiles\Deck\Deck.exe" -ArgumentList "--cdp-port=9222"
