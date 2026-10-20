# PulseTerm echo latency kit: Windows Terminal (native) vs Edge + xterm.js 6 (DOM and WebGL).
# Usage: powershell -ExecutionPolicy Bypass -File .\run.ps1 [-Samples 100] [-Rounds 2]
param([int]$Samples = 100, [int]$Rounds = 2)
$ErrorActionPreference = 'Stop'
$kit = $PSScriptRoot
$out = Join-Path $kit ('results-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $out | Out-Null
$warmup = 5
$measure = Join-Path $kit 'echo-latency.exe'
$echo = Join-Path $kit 'echo-console.exe'
$edgeProfile = Join-Path $env:TEMP 'pulseterm-echo-edge'
$page = ([Uri](Join-Path $kit 'echo.html')).AbsoluteUri

function Measure-Target([string]$Label, [string]$Expect, [string]$Title) {
    & $measure --label $Label --expect $Expect --title $Title --samples $Samples --warmup $warmup --countdown 4 --out (Join-Path $out "$Label.json")
    if ($LASTEXITCODE -ne 0) { throw "measurement failed: $Label (exit $LASTEXITCODE)" }
}

# Only Edge processes started with this kit's private profile are stopped.
function Stop-KitEdge {
    Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" |
        Where-Object { $_.CommandLine -like "*pulseterm-echo-edge*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

try {
    for ($round = 1; $round -le $Rounds; $round++) {
        Start-Process wt.exe -ArgumentList "-w new new-tab --title echo-wt --suppressApplicationTitle `"$echo`" $($Samples + $warmup)"
        Measure-Target "r$round-wt" 'WindowsTerminal.exe' 'echo-wt'
        Start-Sleep -Seconds 3
        foreach ($renderer in 'dom', 'webgl') {
            Start-Process msedge.exe -ArgumentList @("--user-data-dir=`"$edgeProfile`"", '--no-first-run', '--no-default-browser-check', '--window-size=1000,700', "--app=${page}?renderer=$renderer")
            try { Measure-Target "r$round-edge-$renderer" 'msedge.exe' "echo-$renderer" }
            finally { Stop-KitEdge; Start-Sleep -Seconds 3 }
        }
    }
} finally {
    Stop-KitEdge
    Start-Sleep -Seconds 1
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $edgeProfile
}

$results = Get-ChildItem $out -Filter 'r*.json' | ForEach-Object { Get-Content $_.FullName -Raw | ConvertFrom-Json }
foreach ($r in $results) {
    if ($r.title -match '-(failed|lost)$') { throw "renderer did not run as requested: $($r.label) $($r.title)" }
}
function Percentile([double[]]$Values, [int]$Percent) {
    $sorted = $Values | Sort-Object
    $sorted[[math]::Ceiling($sorted.Count * $Percent / 100) - 1]
}
$groups = $results | Group-Object { $_.label -replace '^r\d+-', '' }
$rows = foreach ($g in $groups) {
    $values = [double[]]($g.Group | ForEach-Object { $_.latencyMs })
    [pscustomobject]@{ target = $g.Name; samples = $values.Count; p50Ms = Percentile $values 50; p95Ms = Percentile $values 95 }
}
$native = $rows | Where-Object target -eq 'wt'
foreach ($row in $rows) {
    $row | Add-Member p50DeltaVsWtMs ([math]::Round($row.p50Ms - $native.p50Ms, 3))
    $row | Add-Member p95DeltaVsWtMs ([math]::Round($row.p95Ms - $native.p95Ms, 3))
}
$first = $results | Select-Object -First 1
[pscustomobject]@{
    refreshHz = $first.refreshHz; adapter = $first.adapter; windowsBuild = $first.windowsBuild; rows = $rows
} | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $out 'summary.json')
$rows | Format-Table -AutoSize
Compress-Archive -Path $out -DestinationPath "$out.zip"
Write-Host "Send this file back: $out.zip"
