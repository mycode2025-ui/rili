param(
    [switch]$UpdateBaselines
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$currentDir = Join-Path $projectRoot 'target\visual-regression\current'
$baselineDir = Join-Path $projectRoot 'tests\visual\baseline'
New-Item -ItemType Directory -Force -Path $currentDir | Out-Null
New-Item -ItemType Directory -Force -Path $baselineDir | Out-Null

if (-not (Get-Command slint-viewer -ErrorAction SilentlyContinue)) {
    throw '未找到 slint-viewer，请先安装与工程版本一致的 Slint 工具。'
}

$components = @(
    'CalendarWidgetWindow', 'EventsWidgetWindow', 'CountdownWidgetWindow',
    'ClockWidgetWindow', 'WeatherWidgetWindow', 'FocusWidgetWindow',
    'TodoWidgetWindow', 'NotesWidgetWindow'
)
$scales = @('1.0', '1.25', '1.5', '2.0')

Push-Location $projectRoot
try {
    foreach ($scale in $scales) {
        $tag = $scale.Replace('.', '')
        $env:SLINT_SCALE_FACTOR = $scale
        foreach ($component in $components) {
            $output = Join-Path $currentDir "$component-$tag.png"
            $arguments = @('ui\desktop-widgets.slint', '--component', $component, '--screenshot', $output)
            if ($component -eq 'WeatherWidgetWindow') {
                $arguments += @('--load-data', 'tests\visual\weather.json')
            }
            & slint-viewer @arguments
            if ($LASTEXITCODE -ne 0) { throw "渲染失败：$component @ $scale" }
        }
        $quickOutput = Join-Path $currentDir "QuickPanelWindow-$tag.png"
        & slint-viewer ui\quick-panel.slint --component QuickPanelWindow --load-data tests\visual\quick-panel.json --screenshot $quickOutput
        if ($LASTEXITCODE -ne 0) { throw "渲染失败：QuickPanelWindow @ $scale" }
    }
} finally {
    Remove-Item Env:SLINT_SCALE_FACTOR -ErrorAction SilentlyContinue
    Pop-Location
}

if ($UpdateBaselines) {
    Copy-Item (Join-Path $currentDir '*.png') $baselineDir -Force
    Write-Host '已更新 36 张视觉基线。'
    exit 0
}

Add-Type -AssemblyName System.Drawing
$failures = [System.Collections.Generic.List[string]]::new()
foreach ($current in Get-ChildItem $currentDir -Filter '*.png') {
    $baselinePath = Join-Path $baselineDir $current.Name
    if (-not (Test-Path $baselinePath)) {
        $failures.Add("缺少基线：$($current.Name)")
        continue
    }
    $actual = [System.Drawing.Bitmap]::FromFile($current.FullName)
    $expected = [System.Drawing.Bitmap]::FromFile($baselinePath)
    try {
        if ($actual.Width -ne $expected.Width -or $actual.Height -ne $expected.Height) {
            $failures.Add("尺寸变化：$($current.Name) $($expected.Width)x$($expected.Height) -> $($actual.Width)x$($actual.Height)")
            continue
        }
        $stepX = [Math]::Max(1, [Math]::Floor($actual.Width / 160))
        $stepY = [Math]::Max(1, [Math]::Floor($actual.Height / 160))
        $different = 0
        $sampled = 0
        for ($y = 0; $y -lt $actual.Height; $y += $stepY) {
            for ($x = 0; $x -lt $actual.Width; $x += $stepX) {
                $a = $actual.GetPixel($x, $y)
                $b = $expected.GetPixel($x, $y)
                $sampled++
                if ([Math]::Abs($a.R - $b.R) + [Math]::Abs($a.G - $b.G) + [Math]::Abs($a.B - $b.B) -gt 36) {
                    $different++
                }
            }
        }
        $ratio = $different / [double]$sampled
        if ($ratio -gt 0.02) {
            $failures.Add("像素差异：$($current.Name) $([Math]::Round($ratio * 100, 2))%")
        }
    } finally {
        $actual.Dispose()
        $expected.Dispose()
    }
}

if ($failures.Count -gt 0) {
    $failures | ForEach-Object { Write-Error $_ }
    throw "视觉回归失败，共 $($failures.Count) 项。"
}
Write-Host '视觉回归通过：9 个窗口 × 4 档 DPI，共 36 张截图。'
