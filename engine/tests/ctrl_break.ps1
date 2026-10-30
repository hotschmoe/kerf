# Windows only: Ctrl-Break to a running `kerf serve` (own process group, the console is shared) must stop the active
# agent tree and exit with 130. Run: powershell -NoProfile -File tests\ctrl_break.ps1 <kerf.exe> <node.exe> <fake-agent.mjs>
param([string]$Kerf, [string]$Node, [string]$Fake)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class K {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct SI { public int cb; public string r, d, t; public int x, y, xs, ys, xc, yc, fa, fl; public short sw, cr; public IntPtr rp, si, so, se; }
  [StructLayout(LayoutKind.Sequential)]
  public struct PI { public IntPtr hp, ht; public int pid, tid; }
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern bool CreateProcessW(string app, string cmd, IntPtr pa, IntPtr ta, bool inh, uint flags, IntPtr env, string cwd, ref SI si, out PI pi);
  [DllImport("kernel32.dll")] public static extern bool GenerateConsoleCtrlEvent(uint ev, uint grp);
  [DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(IntPtr h, bool add);
  [DllImport("kernel32.dll")] public static extern uint WaitForSingleObject(IntPtr h, uint ms);
  [DllImport("kernel32.dll")] public static extern bool GetExitCodeProcess(IntPtr h, out uint code);
}
'@
$work = Join-Path $env:TEMP ('kerf-brk-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path "$work\.kerf" | Out-Null
$agents = @{ agents = @(@{ id = 'fake'; detect = @($Node, '--version'); argv = @($Node, $Fake, '{message}', '{resume}'); resume = @('--resume', '{session_id}') }) } | ConvertTo-Json -Depth 6
Set-Content -Path "$work\.kerf\agents.json" -Value $agents -Encoding ascii
$port = 47000 + (Get-Random -Maximum 900)
$si = New-Object K+SI; $si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si)
$pi = New-Object K+PI
$cmd = "`"$Kerf`" serve --dir `"$work`" --port $port --no-token --trust-agents"
[void][K]::SetConsoleCtrlHandler([IntPtr]::Zero, $true)  # this script ignores Ctrl-C itself; Ctrl-Break is sent to kerf's group only
if (-not [K]::CreateProcessW([NullString]::Value, $cmd, [IntPtr]::Zero, [IntPtr]::Zero, $true, 0x200, [IntPtr]::Zero, $work, [ref]$si, [ref]$pi)) { throw ('CreateProcess failed: ' + [Runtime.InteropServices.Marshal]::GetLastWin32Error() + ' cb=' + $si.cb + ' cmd=' + $cmd) }
$ok = $true
function Check($name, $cond) { if ($cond) { Write-Output "  ok   $name" } else { Write-Output "  FAIL $name"; $script:ok = $false } }
try {
  for ($i = 0; $i -lt 50; $i++) { try { $null = Invoke-RestMethod "http://127.0.0.1:$port/api/info"; break } catch { Start-Sleep -Milliseconds 100 } }
  $body = '{"agent":"fake","message":"TREE ctrl-break"}'
  $run = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:$port/api/agent/run" -Body $body -ContentType 'application/json'
  Start-Sleep -Milliseconds 1500
  $kids = @(Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq $pi.pid })
  $all = @($kids)
  foreach ($k in $kids) { $all += @(Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq $k.ProcessId }) }
  Check "agent tree is running (agent + grandchild, $($all.Count) processes)" ($all.Count -ge 2)
  [void][K]::GenerateConsoleCtrlEvent(1, [uint32]$pi.pid)
  $w = [K]::WaitForSingleObject($pi.hp, 8000)
  Check 'kerf exits within 8 s of Ctrl-Break' ($w -eq 0)
  $code = 0; [void][K]::GetExitCodeProcess($pi.hp, [ref]$code)
  Check "exit code 130 (got $code)" ($code -eq 130)
  Start-Sleep -Milliseconds 500
  $alive = @($all | Where-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue })
  Check 'agent and grandchild are gone' ($alive.Count -eq 0)
} finally {
  if ((Get-Process -Id $pi.pid -ErrorAction SilentlyContinue)) { Stop-Process -Id $pi.pid -Force }
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
if ($ok) { Write-Output 'ctrl-break: OK'; exit 0 } else { Write-Output 'ctrl-break: FAILED'; exit 1 }
