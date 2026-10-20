# Windows Terminal vs the installed WebView2 Runtime + xterm.js 6 (DOM and WebGL).
# Compatible with Windows PowerShell 5.1. No Edge browser is required.
param(
    [ValidateRange(20, 512)][int]$Samples = 100,
    [ValidateRange(1, 10)][int]$Rounds = 2,
    [string]$RuntimePath,
    [switch]$DiagnoseWebGL
)
$ErrorActionPreference = 'Stop'
Write-Host "echo-kit: webview2-deferred-v4"
$kit = $PSScriptRoot
$measure = Join-Path $kit 'echo-latency.exe'
$echo = Join-Path $kit 'echo-console.exe'
$web = Join-Path $kit 'echo-webview2.exe'
foreach ($file in @($measure, $echo, $web, (Join-Path $kit 'WebView2Loader.dll'), (Join-Path $kit 'echo.html'))) {
    if (!(Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing kit file: $file. Extract the complete new zip." }
}
$wt = (Get-Command wt.exe -CommandType Application -ErrorAction Stop).Source
if ($RuntimePath) {
    $RuntimePath = (Get-Item -LiteralPath $RuntimePath -ErrorAction Stop).FullName
    if (!(Test-Path -LiteralPath (Join-Path $RuntimePath 'msedgewebview2.exe') -PathType Leaf)) {
        throw 'RuntimePath must be a version directory containing msedgewebview2.exe.'
    }
}
$out = Join-Path $kit ('results-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 6))
New-Item -ItemType Directory -Path $out | Out-Null
$warmup = 5
$profileRoot = Join-Path $env:TEMP ('pulseterm-echo-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $profileRoot | Out-Null
$page = ([Uri](Join-Path $kit 'echo.html')).AbsoluteUri
$webWindow = $null

function Measure-Target([string]$Label, [string]$Expect, [string]$Title) {
    & $measure --label $Label --expect $Expect --title $Title --samples $Samples --warmup $warmup --countdown 4 --diagnostic ([int][bool]$DiagnoseWebGL) --out (Join-Path $out "$Label.json")
    if ($LASTEXITCODE -ne 0) { throw "measurement failed: $Label (exit $LASTEXITCODE). Partial results remain in $out" }
}

function Wait-WebView([System.Diagnostics.Process]$Process, [string]$Title, [string]$Log) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        $Process.Refresh()
        if ($Process.HasExited) { throw "WebView2 host exited: $($Process.ExitCode). See $Log" }
        if ($Process.MainWindowTitle -eq $Title) { return }
        if ($Process.MainWindowTitle -match '-(failed|lost)$') { throw "Requested renderer unavailable. See $Log" }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "WebView2 did not become ready within 30 seconds. See $Log"
}

function Stop-KitWebView {
    if ($webWindow) {
        $webWindow.Refresh()
        if (!$webWindow.HasExited) {
            $null = $webWindow.CloseMainWindow()
            if (!$webWindow.WaitForExit(5000)) { $webWindow.Kill(); $webWindow.WaitForExit() }
        }
    }
    # Only terminate browser subprocesses whose command line names this run's unique profile root.
    Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
        Where-Object { $_.CommandLine -and $_.CommandLine.IndexOf($profileRoot, [StringComparison]::OrdinalIgnoreCase) -ge 0 } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

try {
    for ($round = 1; $round -le $Rounds; $round++) {
        if (!$DiagnoseWebGL) {
            Start-Process -FilePath $wt -ArgumentList "-w new new-tab --title echo-wt --suppressApplicationTitle `"$echo`" $($Samples + $warmup)"
            Measure-Target "r$round-wt" 'WindowsTerminal.exe' 'echo-wt'
            Start-Sleep -Seconds 3
        }
        $renderers = if ($DiagnoseWebGL) { @('webgl') } else { @('dom', 'webgl') }
        foreach ($renderer in $renderers) {
            $label = "r$round-webview2-$renderer"
            $profile = Join-Path $profileRoot $label
            $log = Join-Path $out "$label-host.log"
            $arguments = @('--url', "`"${page}?renderer=$renderer`"", '--profile', "`"$profile`"", '--log', "`"$log`"")
            if ($DiagnoseWebGL) { $arguments += @('--diagnostic', '1') }
            if ($RuntimePath) { $arguments += @('--runtime', "`"$RuntimePath`"") }
            $webWindow = Start-Process -FilePath $web -ArgumentList $arguments -PassThru
            try {
                $readyTitle = if ($DiagnoseWebGL) { "echo-$renderer-diagnostic" } else { "echo-$renderer" }
                Wait-WebView $webWindow $readyTitle $log
                Measure-Target $label 'echo-webview2.exe' "echo-$renderer"
            } finally { Stop-KitWebView; $webWindow = $null; Start-Sleep -Seconds 1 }
        }
    }
} catch {
    $failure = $_
    try { Stop-KitWebView } catch { Write-Warning "Host cleanup failed: $_" }
    $webWindow = $null
    $failure | Out-String | Set-Content (Join-Path $out 'failure.txt')
    Compress-Archive -Path "$out\*" -DestinationPath "$out.zip" -Force -ErrorAction Continue
    Write-Host "Send this diagnostic archive back: $out.zip"
    throw $failure
} finally {
    Stop-KitWebView
    Remove-Item -LiteralPath $profileRoot -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $profileRoot) { Write-Warning "Temporary profile could not be removed: $profileRoot" }
}

if ($DiagnoseWebGL) {
    @{ schemaVersion=1; diagnosticOnly=$true; latencyComparisonValid=$false } | ConvertTo-Json | Set-Content (Join-Path $out 'summary.json')
    Compress-Archive -Path "$out\*" -DestinationPath "$out.zip" -Force
    Write-Host "Diagnostic logging changes timing. Send this archive back: $out.zip"
    return
}
$results = @(Get-ChildItem $out -Filter 'r*.json' | ForEach-Object { Get-Content $_.FullName -Raw | ConvertFrom-Json })
if ($results.Count -ne 3 * $Rounds) { throw 'Missing target results; no complete summary was generated.' }
$first = $results[0]
foreach ($r in $results) {
    if ($r.title -match '-(failed|lost)$') { throw "renderer did not run as requested: $($r.label)" }
    if ($r.presentTimeMissing -ne 0) { throw "Missing DXGI present timestamps: $($r.label)" }
    if ($r.adapter -ne $first.adapter -or $r.refreshHz -ne $first.refreshHz -or $r.displayFrequency -ne $first.displayFrequency) {
        throw 'Display/GPU changed between targets. Do not combine these results; rerun on one display.'
    }
}
function Percentile([double[]]$Values, [int]$Percent) {
    $sorted = $Values | Sort-Object
    $sorted[[math]::Ceiling($sorted.Count * $Percent / 100) - 1]
}
$groups = $results | Group-Object { $_.label -replace '^r\d+-', '' }
$rows = @(foreach ($g in $groups) {
    $values = [double[]]($g.Group | ForEach-Object { $_.latencyMs })
    [pscustomobject]@{ target = $g.Name; samples = $values.Count; p50Ms = Percentile $values 50; p95Ms = Percentile $values 95 }
})
$native = $rows | Where-Object target -eq 'wt'
foreach ($row in $rows) {
    $row | Add-Member -NotePropertyName p50DeltaVsWtMs -NotePropertyValue ([math]::Round($row.p50Ms - $native.p50Ms, 3))
    $row | Add-Member -NotePropertyName p95DeltaVsWtMs -NotePropertyValue ([math]::Round($row.p95Ms - $native.p95Ms, 3))
}
[pscustomobject]@{
    refreshHz = $first.refreshHz; adapter = $first.adapter; windowsBuild = $first.windowsBuild; rows = $rows
} | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $out 'summary.json')
$rows | Format-Table -AutoSize
Compress-Archive -Path $out -DestinationPath "$out.zip"
Write-Host "Send this file back: $out.zip"
