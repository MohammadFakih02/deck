param([string]$combo)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class K { [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra); }
"@
$vks = $combo -split '\+' | ForEach-Object { switch ($_) { 'ctrl' {0x11} 'alt' {0x12} 'shift' {0x10} 'space' {0x20} 'win' {0x5B} default { [byte][char]$_.ToUpper() } } }
foreach ($v in $vks) { [K]::keybd_event([byte]$v, 0, 0, [UIntPtr]::Zero) }
Start-Sleep -Milliseconds 60
[array]::Reverse($vks); foreach ($v in $vks) { [K]::keybd_event([byte]$v, 0, 2, [UIntPtr]::Zero) }
