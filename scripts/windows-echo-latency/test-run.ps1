param([string]$Script = (Join-Path $PSScriptRoot 'run.ps1'), [Parameter(Mandatory=$true)][string]$TestRoot)
$ErrorActionPreference = 'Stop'
$tokens = $null; $errors = $null
$null = [System.Management.Automation.Language.Parser]::ParseFile($Script, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
if ([IO.File]::ReadAllBytes($Script) | Where-Object { $_ -gt 127 }) { throw 'runner must remain ASCII' }
# Run the real orchestration, mocking only Windows operations and the native measurement call.
$mocked = [IO.File]::ReadAllText($Script).Replace('& $measure ', 'Invoke-TestMeasurement ')
New-Item -ItemType Directory -Force $TestRoot | Out-Null
Copy-Item (Join-Path (Split-Path $Script) 'startup-diagnostics.ps1') $TestRoot
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
function Get-Command {
    param($Name, $CommandType, $ErrorAction)
    if ($global:EchoRunnerTest.noWt) { throw 'diagnostics unexpectedly required Windows Terminal' }
    [pscustomobject]@{ Source = '/fixture/wt.exe' }
}
function Start-Process {
    param($FilePath, $ArgumentList, [switch]$PassThru)
    $global:EchoRunnerTest.launches.Add([pscustomobject]@{ File = $FilePath; Arguments = @($ArgumentList) })
    if ($PassThru) {
        $log = [string]$ArgumentList[[Array]::IndexOf($ArgumentList, '--log') + 1]
        ('runtime_image=' + (Join-Path $TestRoot 'runtime-actual/msedgewebview2.exe')) | Set-Content -LiteralPath $log.Trim('"')
        if ($global:EchoRunnerTest.launchFailure) { throw 'fixture launch denied' }
        if ($global:EchoRunnerTest.hostFailure -or ($global:EchoRunnerTest.failRenderer -and ($ArgumentList -join ' ') -match ('renderer=' + $global:EchoRunnerTest.failRenderer))) {
            $process = [Diagnostics.Process]::Start('/bin/false')
            $process.WaitForExit()
        } else { $process = [Diagnostics.Process]::Start('/bin/sleep', '60') }
        $title = if (($ArgumentList -join ' ') -match '--startup-memory 1') { 'echo-startup-ready' } elseif (($ArgumentList -join ' ') -match 'renderer=webgl') { 'echo-webgl' } else { 'echo-dom' }
        if (($ArgumentList -join ' ') -match '--diagnostic 1') { $title += '-diagnostic' }
        $process | Add-Member -NotePropertyName MainWindowTitle -NotePropertyValue $title -Force
        $process | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { $this.Kill(); return $true } -Force
        return $process
    }
}
function Start-Sleep { param($Seconds, $Milliseconds) }
function Get-CimInstance { param($ClassName, $Filter) @() }
function Get-Acl {
    param($LiteralPath, $ErrorAction)
    if ($LiteralPath -like '*runtime-actual*') { Write-Error 'fixture ACL denied' -ErrorId 'FixtureAclDenied' -ErrorAction Stop }
    [pscustomobject]@{ Sddl='fixture-sddl' }
}
function Get-WinEvent {
    param($FilterHashtable, $MaxEvents, $ErrorAction)
    if ($MaxEvents -ne 128 -or !$FilterHashtable.StartTime -or !$FilterHashtable.EndTime) { throw 'unbounded event query' }
    if ($FilterHashtable.LogName -like '*CodeIntegrity*') { Write-Error 'fixture denied' -ErrorId 'FixtureDenied' -ErrorAction Stop }
    if ($FilterHashtable.LogName -like '*AppLocker*') { Write-Error 'fixture empty' -ErrorId 'NoMatchingEventsFound' -ErrorAction Stop }
    foreach ($app in 'unrelated.exe', 'msedgewebview2.exe') {
        $event = [pscustomobject]@{ Id=1000; ProviderName='fixture'; TimeCreated=(Get-Date); App=$app }
        $event | Add-Member -MemberType ScriptMethod -Name ToXml -Value {
            return '<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event"><EventData><Data Name="AppName">' + $this.App + '</Data><Data Name="ExceptionCode">c0000022</Data><Data Name="CommandLine">do-not-export-secret</Data></EventData></Event>'
        }
        $event
    }
}
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
$global:EchoRunnerTest | Add-Member -NotePropertyName noWt -NotePropertyValue $true
$before = $global:EchoRunnerTest.measurements
$launchesBefore = $global:EchoRunnerTest.launches.Count
try { & $runner -Rounds 1 -DiagnoseStartup; throw 'missing startup control failure' }
catch { if ($_.Exception.Message -notlike 'Startup probes failed*') { throw } }
if ($global:EchoRunnerTest.launches.Count -ne $launchesBefore + 3) { throw 'startup failure stopped independent controls' }
$startup = Get-ChildItem $TestRoot -Recurse -Filter startup-summary.json | Select-Object -First 1
$probes = Get-Content $startup.FullName -Raw | ConvertFrom-Json
if ($probes.probes.Count -ne 3 -or @($probes.probes | Where-Object ready).Count) { throw 'failed startup results lost' }
if (!(Test-Path ($startup.Directory.FullName + '.zip'))) { throw 'startup failure archive missing' }
$systemFile = Join-Path $startup.Directory.FullName 'startup-system.json'
$system = Get-Content $systemFile -Raw | ConvertFrom-Json
if ($system.acls.Count -lt 5 -or $system.events.Count -ne 2) { throw 'system evidence missing or unrelated events included' }
if (@($system.acls | Where-Object { $_.path -like '*runtime-actual*' -and $_.status -eq 'unavailable' }).Count -ne 2) { throw 'actual runtime ancestry or ACL denial missing' }
if (($system.events[0].data | Where-Object name -eq 'ExceptionCode').value -ne 'c0000022') { throw 'event fields not extracted' }
if (($system.eventQueries | Where-Object log -like '*CodeIntegrity*').status -ne 'unavailable') { throw 'event permission denial not reported' }
if (($system.eventQueries | Where-Object log -like '*AppLocker*').status -ne 'no-events') { throw 'empty event query not distinguished' }
if ((Get-Content $systemFile -Raw) -match 'do-not-export-secret|unrelated.exe') { throw 'unapproved event data exported' }
$global:EchoRunnerTest.hostFailure = $false
$global:EchoRunnerTest | Add-Member -NotePropertyName launchFailure -NotePropertyValue $true
$launchesBefore = $global:EchoRunnerTest.launches.Count
try { & $runner -Rounds 1 -DiagnoseStartup; throw 'missing launch failure' }
catch { if ($_.Exception.Message -notlike 'Startup probes failed*') { throw } }
if ($global:EchoRunnerTest.launches.Count -ne $launchesBefore + 3) { throw 'launch failure stopped independent controls' }
$global:EchoRunnerTest.launchFailure = $false
$global:EchoRunnerTest | Add-Member -NotePropertyName failRenderer -NotePropertyValue 'webgl'
try { & $runner -Rounds 1 -DiagnoseStartup; throw 'missing mixed startup failure' }
catch { if ($_.Exception.Message -notlike 'Startup probes failed*') { throw } }
$global:EchoRunnerTest.failRenderer = $null
& $runner -Rounds 1 -DiagnoseStartup
$allStartup = @(Get-ChildItem $TestRoot -Recurse -Filter startup-summary.json)
$readyCounts = @($allStartup | ForEach-Object { @((Get-Content $_.FullName -Raw | ConvertFrom-Json).probes | Where-Object ready).Count } | Sort-Object)
if (($readyCounts -join ',') -ne '0,0,2,3') { throw 'all-failed, mixed, and successful startup outcomes not preserved' }
$controls = @($global:EchoRunnerTest.launches | Where-Object { ($_.Arguments -join ' ') -match '--startup-memory 1' })
if ($controls.Count -ne 4 -or @( $controls | Where-Object { ($_.Arguments -join ' ') -match '--diagnostic' }).Count) { throw 'memory control enabled xterm diagnostics' }
if ($global:EchoRunnerTest.measurements -ne $before) { throw 'startup controls performed sampling' }
if (Get-ChildItem $TestRoot -Directory -Filter 'pulseterm-echo-*') { throw 'startup profiles not removed' }
try { & $runner -DiagnoseWebGL -DiagnoseStartup; throw 'conflicting diagnostic modes accepted' }
catch { if ($_.Exception.Message -notlike 'Choose DiagnoseWebGL*') { throw } }
Write-Host 'runner: existing measurement flow plus independent startup controls, evidence filtering, failures, archives and cleanup passed (mock Windows operations)'
