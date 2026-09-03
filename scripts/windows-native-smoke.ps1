param(
    [string]$ExePath = "target\debug\rili.exe",
    [int]$TimeoutSeconds = 15
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$resolvedExe = [System.IO.Path]::GetFullPath((Join-Path $projectRoot $ExePath))
if (-not (Test-Path -LiteralPath $resolvedExe)) {
    throw "TimeHub executable not found: $resolvedExe"
}

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class TimeHubNativeSmoke {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
    public delegate bool EnumMonitorsProc(IntPtr monitor, IntPtr hdc, IntPtr rect, IntPtr data);

    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    public struct MONITORINFOEX {
        public int cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string szDevice;
    }
    public sealed class WindowInfo { public IntPtr Handle; public string Title; public RECT Rect; }
    public sealed class MonitorInfo { public string Device; public RECT Monitor; public RECT Work; }

    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern int GetWindowTextLength(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, EnumMonitorsProc callback, IntPtr data);
    [DllImport("user32.dll", CharSet = CharSet.Auto)] static extern bool GetMonitorInfo(IntPtr monitor, ref MONITORINFOEX info);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);

    public static List<WindowInfo> Windows(uint pid) {
        var result = new List<WindowInfo>();
        EnumWindows((hwnd, _) => {
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            if (owner == pid && IsWindowVisible(hwnd)) {
                var length = GetWindowTextLength(hwnd);
                var text = new StringBuilder(length + 1);
                GetWindowText(hwnd, text, text.Capacity);
                RECT rect;
                if (GetWindowRect(hwnd, out rect)) result.Add(new WindowInfo { Handle = hwnd, Title = text.ToString(), Rect = rect });
            }
            return true;
        }, IntPtr.Zero);
        return result;
    }

    public static List<MonitorInfo> Monitors() {
        var result = new List<MonitorInfo>();
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (monitor, _, __, ___) => {
            var info = new MONITORINFOEX();
            info.cbSize = Marshal.SizeOf(info);
            if (GetMonitorInfo(monitor, ref info)) result.Add(new MonitorInfo { Device = info.szDevice, Monitor = info.rcMonitor, Work = info.rcWork });
            return true;
        }, IntPtr.Zero);
        return result;
    }
}
'@

function Get-TimeHubWindows([int]$ProcessId) {
    return [TimeHubNativeSmoke]::Windows([uint32]$ProcessId)
}

function Wait-Window([int]$ProcessId, [string]$Title) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $match = Get-TimeHubWindows $ProcessId | Where-Object Title -eq $Title | Select-Object -First 1
        if ($null -ne $match) { return $match }
        Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Timed out waiting for native window '$Title'."
}

function Assert-InWorkArea($Window, $Monitors) {
    $rect = $Window.Rect
    $inside = $Monitors | Where-Object {
        $work = $_.Work
        $rect.Left -ge ($work.Left - 2) -and $rect.Top -ge ($work.Top - 2) -and
        $rect.Right -le ($work.Right + 2) -and $rect.Bottom -le ($work.Bottom + 2)
    } | Select-Object -First 1
    if ($null -eq $inside) {
        throw "Window '$($Window.Title)' is outside every monitor work area: $($rect.Left),$($rect.Top),$($rect.Right),$($rect.Bottom)"
    }
    return $inside
}

$ownedProcesses = Get-Process -ErrorAction SilentlyContinue | Where-Object {
    try {
        $candidatePath = [System.IO.Path]::GetFullPath($_.MainModule.FileName)
        $candidateName = [System.IO.Path]::GetFileNameWithoutExtension($candidatePath)
        $candidatePath -eq $resolvedExe -or $candidateName -in @('rili', 'TimeHub')
    } catch { $false }
}
foreach ($owned in $ownedProcesses) {
    Stop-Process -Id $owned.Id -Force
    [void]$owned.WaitForExit(5000)
}

$previousSmoke = $env:TIMEHUB_NATIVE_SMOKE
$env:TIMEHUB_NATIVE_SMOKE = '1'
$process = $null
try {
    $process = Start-Process -FilePath $resolvedExe -WorkingDirectory $projectRoot -PassThru
    $main = Wait-Window $process.Id '日历'
    $quick = Wait-Window $process.Id 'TimeHub 快速面板'
    $notification = Wait-Window $process.Id 'TimeHub 通知'

    $monitors = [TimeHubNativeSmoke]::Monitors()
    if ($monitors.Count -lt 1) { throw 'Windows reported no active monitors.' }
    $quickMonitor = Assert-InWorkArea $quick $monitors
    $notificationMonitor = Assert-InWorkArea $notification $monitors

    $rightGap = $notificationMonitor.Work.Right - $notification.Rect.Right
    $bottomGap = $notificationMonitor.Work.Bottom - $notification.Rect.Bottom
    if ($rightGap -gt 96 -or $bottomGap -gt 96) {
        throw "Notification is not anchored at the screen work-area bottom-right (right=$rightGap, bottom=$bottomGap)."
    }

    Start-Sleep -Milliseconds 1300
    $quickAfterClock = Wait-Window $process.Id 'TimeHub 快速面板'
    [void](Assert-InWorkArea $quickAfterClock $monitors)

    $second = Start-Process -FilePath $resolvedExe -WorkingDirectory $projectRoot -PassThru
    if (-not $second.WaitForExit(5000)) {
        Stop-Process -Id $second.Id -Force
        throw 'Second TimeHub instance did not exit.'
    }
    $matching = Get-Process -ErrorAction SilentlyContinue | Where-Object {
        try { [System.IO.Path]::GetFullPath($_.MainModule.FileName) -eq $resolvedExe } catch { $false }
    }
    if (@($matching).Count -ne 1) { throw "Expected one TimeHub instance, found $(@($matching).Count)." }

    $crossMonitor = 'skipped (single monitor)'
    if ($monitors.Count -gt 1) {
        $original = $main.Rect
        foreach ($monitor in $monitors) {
            $work = $monitor.Work
            $width = [Math]::Min(1100, $work.Right - $work.Left - 40)
            $height = [Math]::Min(720, $work.Bottom - $work.Top - 40)
            [void][TimeHubNativeSmoke]::SetWindowPos($main.Handle, [IntPtr]::Zero, $work.Left + 20, $work.Top + 20, $width, $height, 0x0040)
            Start-Sleep -Milliseconds 500
            $moved = Get-TimeHubWindows $process.Id | Where-Object Title -eq '日历' | Select-Object -First 1
            [void](Assert-InWorkArea $moved @($monitor))
        }
        [void][TimeHubNativeSmoke]::SetWindowPos($main.Handle, [IntPtr]::Zero, $original.Left, $original.Top, $original.Right - $original.Left, $original.Bottom - $original.Top, 0x0040)
        $crossMonitor = "passed ($($monitors.Count) monitors)"
    }

    Write-Output "Windows native smoke passed: tray handler, taskbar-clock handler, screen notification placement, single instance; multi-monitor $crossMonitor."
}
finally {
    if ($null -ne $process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
        [void]$process.WaitForExit(5000)
    }
    if ($null -eq $previousSmoke) { Remove-Item Env:TIMEHUB_NATIVE_SMOKE -ErrorAction SilentlyContinue }
    else { $env:TIMEHUB_NATIVE_SMOKE = $previousSmoke }
}
