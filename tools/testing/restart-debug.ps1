Start-Process "$env:ProgramFiles\Deck\Deck.exe" -ArgumentList "--quit"
for ($i = 0; $i -lt 40 -and (Get-Process Deck -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
Start-Process "$env:ProgramFiles\Deck\Deck.exe"
