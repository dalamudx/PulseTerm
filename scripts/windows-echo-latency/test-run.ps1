param([string]$Script = (Join-Path $PSScriptRoot 'run.ps1'), [Parameter(Mandatory=$true)][string]$TestRoot)
$ErrorActionPreference = 'Stop'
$tokens = $null; $errors = $null
$null = [System.Management.Automation.Language.Parser]::ParseFile($Script, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
if ([IO.File]::ReadAllBytes($Script) | Where-Object { $_ -gt 127 }) { throw 'runner must remain ASCII' }
# Run the real orchestration, mocking only Windows operations and the native measurement call.
$mocked = [IO.File]::ReadAllText($Script).Replace('& $measure ', 'Invoke-TestMeasurement ')
New-Item -ItemType Directory -Force $TestRoot | Out-Null
$runner = Join-Path $TestRoot 'run.ps1'
[IO.File]::WriteAllText($runner, $mocked)
foreach ($name in 'echo-latency.exe','echo-console.exe','echo-webview2.exe','WebView2Loader.dll','echo.html') {
    [IO.File]::WriteAllText((Join-Path $TestRoot $name), 'synthetic fixture')
}
$runtime = Join-Path $TestRoot 'runtime with spaces'
New-Item -ItemType Directory $runtime | Out-Null
[IO.File]::WriteAllText((Join-Path $runtime 'msedgewebview2.exe'), 'synthetic fixture')
$env:TEMP = $TestRoot
$global:EchoRunnerTest = [pscustomobject]@{ launches = [Collections.Generic.List[object]]::new(); measurements = 0; mismatch = $false }
function Get-Command { param($Name, $CommandType, $ErrorAction) [pscustomobject]@{ Source = '/fixture/wt.exe' } }
function Start-Process {
    param($FilePath, $ArgumentList, [switch]$PassThru)
    $global:EchoRunnerTest.launches.Add([pscustomobject]@{ File = $FilePath; Arguments = @($ArgumentList) })
    if ($PassThru) {
        if ($global:EchoRunnerTest.hostFailure) {
            $process = [Diagnostics.Process]::Start('/bin/false')
            $process.WaitForExit()
        } else { $process = [Diagnostics.Process]::Start('/bin/sleep', '60') }
        $title = if (($ArgumentList -join ' ') -match 'renderer=webgl') { 'echo-webgl' } else { 'echo-dom' }
        if (($ArgumentList -join ' ') -match '--diagnostic 1') { $title += '-diagnostic' }
        $process | Add-Member -NotePropertyName MainWindowTitle -NotePropertyValue $title -Force
        $process | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { $this.Kill(); return $true } -Force
        return $process
    }
}
function Start-Sleep { param($Seconds, $Milliseconds) }
function Get-CimInstance { param($ClassName, $Filter) @() }
function Invoke-TestMeasurement {
    param([Parameter(ValueFromRemainingArguments=$true)][object[]]$Rest)
    $global:EchoRunnerTest.measurements++
    $label = [string]$Rest[[Array]::IndexOf($Rest, '--label') + 1]
    $output = [string]$Rest[[Array]::IndexOf($Rest, '--out') + 1]
    $hz = if ($global:EchoRunnerTest.mismatch -and $label -like '*webgl') { 60 } else { 144 }
    @{ label=$label; title='echo-ok'; adapter='fixture GPU'; refreshHz=$hz; displayFrequency=$hz; windowsBuild='fixture'; presentTimeMissing=0; latencyMs=@(1,2,3,4,5) } | ConvertTo-Json | Set-Content $output
    if ($global:EchoRunnerTest.failNext) { $global:LASTEXITCODE = 4; return }
    $global:LASTEXITCODE = 0
}
& $runner -Rounds 1 -RuntimePath $runtime
if ($global:EchoRunnerTest.measurements -ne 3 -or $global:EchoRunnerTest.launches.Count -ne 3) { throw 'expected WT + two WebView2 launches' }
foreach ($launch in $global:EchoRunnerTest.launches | Select-Object -Skip 1) {
    if ($launch.File -notmatch 'echo-webview2.exe$') { throw 'incorrect host executable' }
    if (($launch.Arguments -join ' ') -notmatch [regex]::Escape('"' + $runtime + '"')) { throw 'runtime quoting lost' }
}
$summaryFile = Get-ChildItem $TestRoot -Recurse -Filter summary.json | Select-Object -First 1
$summary = Get-Content $summaryFile.FullName -Raw | ConvertFrom-Json
if ($summary.rows.Count -ne 3 -or $summary.refreshHz -ne 144) { throw 'summary shape' }
if (($summary.rows | Where-Object { $_.p95Ms -ne 5 -or $_.p95DeltaVsWtMs -ne 0 }).Count) { throw 'aggregation' }
$global:EchoRunnerTest.mismatch = $true
try { & $runner -Rounds 1 -RuntimePath $runtime; throw 'missing display mismatch rejection' }
catch { if ($_.Exception.Message -notlike 'Display/GPU changed*') { throw } }
$before = $global:EchoRunnerTest.measurements
try { & $runner -Rounds 1 -RuntimePath (Join-Path $TestRoot 'missing'); throw 'missing preflight rejection' }
catch { if ($_.Exception.Message -eq 'missing preflight rejection') { throw } }
if ($global:EchoRunnerTest.measurements -ne $before) { throw 'preflight performed measurement' }
$global:EchoRunnerTest.mismatch = $false
$before = $global:EchoRunnerTest.measurements
& $runner -Rounds 1 -RuntimePath $runtime -DiagnoseWebGL
if ($global:EchoRunnerTest.measurements -ne $before + 1) { throw 'diagnostic mode did not isolate WebGL' }
$lastLaunch = $global:EchoRunnerTest.launches[$global:EchoRunnerTest.launches.Count - 1]
if (($lastLaunch.Arguments -join ' ') -notmatch '--diagnostic 1') { throw 'diagnostic host flag missing' }
if (($lastLaunch.Arguments -join ' ') -match '#diagnostic|&diagnostic') { throw 'diagnostic flag changed navigation URL' }
$global:EchoRunnerTest | Add-Member -NotePropertyName failNext -NotePropertyValue $true
try { & $runner -Rounds 1 -RuntimePath $runtime -DiagnoseWebGL; throw 'missing measurement failure' }
catch { if ($_.Exception.Message -notlike 'measurement failed:*') { throw } }
$failureLog = Get-ChildItem $TestRoot -Recurse -Filter failure.txt | Select-Object -First 1
if (!$failureLog -or !(Test-Path ($failureLog.Directory.FullName + '.zip'))) { throw 'failure archive missing' }
$global:EchoRunnerTest | Add-Member -NotePropertyName hostFailure -NotePropertyValue $true
$before = $global:EchoRunnerTest.measurements
$beforeFailures = @(Get-ChildItem $TestRoot -Recurse -Filter failure.txt).Count
try { & $runner -Rounds 1 -RuntimePath $runtime -DiagnoseWebGL; throw 'missing host startup failure' }
catch { if ($_.Exception.Message -notlike 'WebView2 host exited:*') { throw } }
if ($global:EchoRunnerTest.measurements -ne $before) { throw 'host startup failure performed sampling' }
$allFailures = @(Get-ChildItem $TestRoot -Recurse -Filter failure.txt)
if ($allFailures.Count -ne $beforeFailures + 1) { throw 'host startup failure was not archived' }
foreach ($item in $allFailures) {
    if (!(Test-Path ($item.Directory.FullName + '.zip'))) { throw 'failure archive missing' }
}
if (Get-ChildItem $TestRoot -Directory -Filter 'pulseterm-echo-*') { throw 'profile not removed' }
Write-Host 'runner: parsing, no-Edge launch flow, quoted RuntimePath, summary, display mismatch and preflight passed (mock Windows operations)'
