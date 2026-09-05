param(
    [switch]$UpdateBaselines
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$currentDir = Join-Path $projectRoot 'target\visual-regression\current'
$baselineDir = Join-Path $projectRoot 'tests\visual\baseline'
New-Item -ItemType Directory -Force -Path $currentDir | Out-Null
New-Item -ItemType Directory -Force -Path $baselineDir | Out-Null
# Current screenshots are generated artifacts. Clear them first so temporary
# inspection images cannot silently become part of the regression inventory.
Get-ChildItem -LiteralPath $currentDir -Filter '*.png' -File | Remove-Item -Force

if (-not (Get-Command slint-viewer -ErrorAction SilentlyContinue)) {
    throw '未找到 slint-viewer，请先安装与工程版本一致的 Slint 工具。'
}

$components = @(
    'CalendarWidgetWindow', 'EventsWidgetWindow', 'CountdownWidgetWindow',
    'ClockWidgetWindow', 'WeatherWidgetWindow', 'FocusWidgetWindow',
    'TodoWidgetWindow', 'NotesWidgetWindow', 'DailyQuoteWidgetWindow',
    'AlmanacWidgetWindow'
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

        $sidePanelOutput = Join-Path $currentDir "SidePanelPreview-$tag.png"
        & slint-viewer tests\visual\side-panel-preview.slint --component SidePanelPreview --screenshot $sidePanelOutput
        if ($LASTEXITCODE -ne 0) { throw "渲染失败：SidePanelPreview @ $scale" }
        foreach ($page in @('today', 'tools', 'records', 'notes')) {
            foreach ($theme in @('dark', 'light')) {
                $mainOutput = Join-Path $currentDir "Main-$page-$theme-$tag.png"
                & slint-viewer tests\visual\main-review-preview.slint --load-data "tests\visual\main-$page-$theme.json" --screenshot $mainOutput
                if ($LASTEXITCODE -ne 0) { throw "渲染失败：Main $page $theme @ $scale" }
            }
        }
    }

    $env:SLINT_SCALE_FACTOR = '1.0'
    & slint-viewer tests\visual\subscription-states-preview.slint --screenshot (Join-Path $currentDir 'SubscriptionStates-dark.png')
    if ($LASTEXITCODE -ne 0) { throw '渲染失败：SubscriptionStates dark' }
    & slint-viewer tests\visual\today-preview.slint --component TodayPreview --screenshot (Join-Path $currentDir 'TodayPreview-light.png')
    if ($LASTEXITCODE -ne 0) { throw '渲染失败：TodayPreview light' }
    & slint-viewer tests\visual\today-preview.slint --component TodayPreview --load-data tests\visual\today-dark.json --screenshot (Join-Path $currentDir 'TodayPreview-dark.png')
    if ($LASTEXITCODE -ne 0) { throw '渲染失败：TodayPreview dark' }
    & slint-viewer tests\visual\tools-dingtalk-preview.slint --component ToolsDingTalkPreview --screenshot (Join-Path $currentDir 'ToolsPreview-light.png')
    if ($LASTEXITCODE -ne 0) { throw '渲染失败：ToolsPreview light' }
    & slint-viewer tests\visual\tools-dingtalk-preview.slint --component ToolsDingTalkPreview --load-data tests\visual\tools-dingtalk-dark.json --screenshot (Join-Path $currentDir 'ToolsPreview-dark.png')
    if ($LASTEXITCODE -ne 0) { throw '渲染失败：ToolsPreview dark' }
    & slint-viewer tests\visual\notes-preview.slint --component NotesPreview --screenshot (Join-Path $currentDir 'NotesPreview-light.png')
    if ($LASTEXITCODE -ne 0) { throw '渲染失败：NotesPreview light' }
    & slint-viewer tests\visual\notes-preview.slint --component NotesPreview --load-data tests\visual\notes-dark.json --screenshot (Join-Path $currentDir 'NotesPreview-dark.png')
    if ($LASTEXITCODE -ne 0) { throw '渲染失败：NotesPreview dark' }
    $env:SLINT_SCALE_FACTOR = '1.5'
    & slint-viewer tests\visual\quick-panel-dark-preview.slint --component QuickPanelDarkPreview --screenshot (Join-Path $currentDir 'QuickPanelPreview-dark-15.png')
    if ($LASTEXITCODE -ne 0) { throw '渲染失败：QuickPanelPreview dark @ 1.5' }
} finally {
    Remove-Item Env:SLINT_SCALE_FACTOR -ErrorAction SilentlyContinue
    Pop-Location
}

if ($UpdateBaselines) {
    Copy-Item (Join-Path $currentDir '*.png') $baselineDir -Force
    Write-Host "已更新 $((Get-ChildItem -LiteralPath $currentDir -Filter '*.png').Count) 张视觉基线；仍需另行运行比较验证。"
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
Write-Host "视觉回归通过：$((Get-ChildItem -LiteralPath $currentDir -Filter '*.png').Count) 张截图，含主窗口四页的明暗主题及四档 DPI。"
