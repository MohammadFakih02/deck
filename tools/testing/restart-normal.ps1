Start-Process "$env:ProgramFiles\Deck\Deck.exe" -ArgumentList "--quit"
for ($i = 0; $i -lt 40 -and (Get-Process Deck -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
schtasks /run /tn "\Deck\Open" | Out-Null
