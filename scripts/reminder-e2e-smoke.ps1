param(
    [string]$ExePath = "target\debug\rili.exe",
    [int]$TimeoutSeconds = 25,
    [ValidateSet('quiet', 'standard', 'strong')]
    [string]$Style = 'standard'
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$resolvedExe = [System.IO.Path]::GetFullPath((Join-Path $projectRoot $ExePath))
$targetRoot = [System.IO.Path]::GetFullPath((Join-Path $projectRoot 'target'))
if (-not (Test-Path -LiteralPath $resolvedExe)) {
    throw "TimeHub executable not found: $resolvedExe"
}

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class TimeHubReminderSmoke {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    [DllImport("user32.dll")] static extern int GetWindowTextLength(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);

    public static bool HasVisibleWindow(uint processId, string title) {
        var found = false;
        EnumWindows((hwnd, _) => {
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            if (owner == processId && IsWindowVisible(hwnd)) {
                var text = new StringBuilder(GetWindowTextLength(hwnd) + 1);
                GetWindowText(hwnd, text, text.Capacity);
                if (text.ToString() == title) found = true;
            }
            return !found;
        }, IntPtr.Zero);
        return found;
    }
}
'@

$smokeData = [System.IO.Path]::GetFullPath((Join-Path $targetRoot ('reminder-smoke-' + [Guid]::NewGuid().ToString('N'))))
if (-not $smokeData.StartsWith($targetRoot + [System.IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing unsafe smoke data path: $smokeData"
}

$previousSmoke = $env:TIMEHUB_NATIVE_SMOKE
$previousSmokeData = $env:TIMEHUB_SMOKE_DATA_DIR
$process = $null
try {
    $env:TIMEHUB_NATIVE_SMOKE = '1'
    $env:TIMEHUB_SMOKE_DATA_DIR = $smokeData
    $now = Get-Date
    & $resolvedExe settings set --key notification_style --value $Style | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Unable to configure reminder smoke style.' }
    & $resolvedExe events create --title '提醒端到端测试' --date $now.ToString('yyyy-MM-dd') --time $now.ToString('HH:mm') --reminder 0 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Unable to create reminder smoke event.' }

    $process = Start-Process -FilePath $resolvedExe -WorkingDirectory $projectRoot -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        if ($process.HasExited) { throw "TimeHub exited before delivering the reminder (code $($process.ExitCode))." }
        if ([TimeHubReminderSmoke]::HasVisibleWindow([uint32]$process.Id, 'TimeHub 通知')) {
            Write-Output "Reminder end-to-end smoke passed: due event -> background scanner -> Slint event loop -> visible popup ($Style)."
            exit 0
        }
        Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Timed out waiting for the visible TimeHub reminder popup ($Style)."
}
finally {
    if ($null -ne $process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
        [void]$process.WaitForExit(5000)
    }
    if ($null -eq $previousSmoke) { Remove-Item Env:TIMEHUB_NATIVE_SMOKE -ErrorAction SilentlyContinue }
    else { $env:TIMEHUB_NATIVE_SMOKE = $previousSmoke }
    if ($null -eq $previousSmokeData) { Remove-Item Env:TIMEHUB_SMOKE_DATA_DIR -ErrorAction SilentlyContinue }
    else { $env:TIMEHUB_SMOKE_DATA_DIR = $previousSmokeData }
    if (Test-Path -LiteralPath $smokeData) {
        $resolvedSmokeData = [System.IO.Path]::GetFullPath($smokeData)
        if ($resolvedSmokeData.StartsWith($targetRoot + [System.IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedSmokeData -Recurse -Force
        }
    }
}
