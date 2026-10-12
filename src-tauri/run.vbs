' Starts a Deck scheduled task without a window: run.vbs "\Deck\Open" or "\Deck\Profile cpu-<id>".
CreateObject("WScript.Shell").Run "schtasks.exe /run /tn """ & WScript.Arguments(0) & """", 0, False
