Start-Process "$env:ProgramFiles\Deck\Deck.exe" -ArgumentList "--quit"
for ($i = 0; $i -lt 40 -and (Get-Process Deck -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
"gone=$(-not (Get-Process Deck -ErrorAction SilentlyContinue)) after $($i*0.5)s" | Out-File "$env:TEMP\deck-quit.txt"
