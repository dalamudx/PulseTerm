param([string]$Collector = (Join-Path $PSScriptRoot 'collect-startup.ps1'), [Parameter(Mandatory=$true)][string]$TestRoot)
$ErrorActionPreference = 'Stop'
$tokens = $null; $errors = $null
$null = [System.Management.Automation.Language.Parser]::ParseFile($Collector, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
if ([IO.File]::ReadAllBytes($Collector) | Where-Object { $_ -gt 127 }) { throw 'collector must remain ASCII' }
New-Item -ItemType Directory -Path $TestRoot | Out-Null
$fixture = Join-Path $TestRoot 'kit with spaces'
New-Item -ItemType Directory -Path $fixture | Out-Null
$script = Join-Path $fixture 'collect-startup.ps1'
# Exercise the real script with Windows operations mocked and observation delays shortened.
$text = [IO.File]::ReadAllText($Collector).Replace('AddSeconds(20)', 'AddSeconds(0)').Replace('.TotalSeconds -ge 3', '.TotalSeconds -ge 0')
[IO.File]::WriteAllText($script, $text)
foreach ($name in 'echo-webview2.exe', 'WebView2Loader.dll') { [IO.File]::WriteAllText((Join-Path $fixture $name), 'fixture') }
$original = @{ args=$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS; temp=$env:TEMP; tmp=$env:TMP; user=$env:USERPROFILE }
$env:USERPROFILE = Join-Path $TestRoot 'user'
$env:TEMP = 'fixture-old-temp'; $env:TMP = 'fixture-old-tmp'; $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--do-not-inherit-this-fixture'

function Get-ProcessMitigation {
    param([switch]$System, $Name, $ErrorAction)
    if ($System) { Write-Error 'fixture denied' -ErrorId 'FixturePolicyDenied' -ErrorAction Stop }
    'fixture policy for ' + $Name
}
function New-FakeProcess([int]$Id) {
    $module = [pscustomobject]@{ ModuleName='fixture.dll'; FileName='C:\fixture\fixture.dll'; FileVersionInfo=[pscustomobject]@{ CompanyName='fixture'; FileVersion='1.0' } }
    $process = [pscustomobject]@{ Id=$Id; HasExited=($global:NativeCollectorTest.mode -ne 'ready' -and $global:NativeCollectorTest.mode -ne 'timeout'); ExitCode=2; MainWindowTitle='echo-startup-ready'; Modules=@($module) }
    if ($global:NativeCollectorTest.mode -eq 'timeout') { $process.MainWindowTitle = 'echo-loading' }
    $process | Add-Member -MemberType ScriptMethod -Name Refresh -Value { }
    $process | Add-Member -MemberType ScriptMethod -Name Dispose -Value { }
    $process | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { $this.HasExited=$true; $this.ExitCode=0; return $true }
    $process | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Timeout) return $this.HasExited }
    $process | Add-Member -MemberType ScriptMethod -Name Kill -Value { $global:NativeCollectorTest.killed += $this.Id; $this.HasExited=$true }
    $process
}
function Start-Process {
    param($FilePath, $ArgumentList, [switch]$PassThru)
    $global:NativeCollectorTest.launches++
    $joined = $ArgumentList -join ' '
    if ($joined -notmatch '--url about:blank --startup-memory 1' -or $joined -match 'renderer|--diagnostic') { throw 'not a fixed memory control' }
    if ($env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -match 'do-not-inherit|no-sandbox|disable-gpu|remote-debugging') { throw 'unsafe or inherited browser switches' }
    if ($env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -notmatch '--enable-logging --v=1 --log-file="([^"]+)"') { throw 'quoted log file missing' }
    $browserLog = $Matches[1]
    $global:NativeCollectorTest.profile = ([string]$ArgumentList[[Array]::IndexOf($ArgumentList, '--profile') + 1]).Trim('"')
    if ($env:TEMP -ne (Split-Path $global:NativeCollectorTest.profile -Parent) -or $env:TMP -ne $env:TEMP) { throw 'temp not local to profile parent' }
    if ($global:NativeCollectorTest.mode -eq 'launch-failed') { throw 'fixture launch failure' }
    if ($global:NativeCollectorTest.mode -ne 'no-log') { 'fixture chromium startup trace' | Set-Content -LiteralPath $browserLog }
    $hostLog = ([string]$ArgumentList[[Array]::IndexOf($ArgumentList, '--log') + 1]).Trim('"')
    'fixture host failure' | Set-Content -LiteralPath $hostLog
    New-FakeProcess 100
}
function Get-CimInstance {
    param($ClassName, $Filter, $ErrorAction)
    if ($global:NativeCollectorTest.mode -eq 'cleanup-unavailable') { Write-Error 'fixture discovery denied' -ErrorId 'FixtureDiscoveryDenied' -ErrorAction Stop }
    [pscustomobject]@{ ProcessId=200; CommandLine=('fixture --user-data-dir=' + $global:NativeCollectorTest.profile) }
    [pscustomobject]@{ ProcessId=999; CommandLine='fixture unrelated browser' }
}
function Get-Process {
    param($Id, $ErrorAction)
    if ($global:NativeCollectorTest.mode -eq 'no-log' -and $Id -eq 200) { Write-Error 'fixture modules denied' -ErrorId 'FixtureModulesDenied' -ErrorAction Stop }
    New-FakeProcess $Id
}
function Start-Sleep { param($Milliseconds) }

try {
    foreach ($mode in @('exited', 'ready', 'timeout', 'no-log', 'launch-failed', 'cleanup-unavailable')) {
        $global:NativeCollectorTest = [pscustomobject]@{ mode=$mode; launches=0; killed=@(); profile=$null }
        $before = @(Get-ChildItem $fixture -Filter native-summary.json -Recurse | ForEach-Object { $_.FullName })
        try {
            & $script
            if ($mode -eq 'launch-failed') { throw 'launch failure not rethrown' }
        } catch { if ($mode -ne 'launch-failed' -or $_.Exception.Message -ne 'fixture launch failure') { throw } }
        if ($global:NativeCollectorTest.launches -ne 1) { throw 'collector launched more than one control' }
        if ($env:TEMP -ne 'fixture-old-temp' -or $env:TMP -ne 'fixture-old-tmp' -or $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ne '--do-not-inherit-this-fixture') { throw 'environment not restored' }
        $new = @(Get-ChildItem $fixture -Filter native-summary.json -Recurse | Where-Object { $_.FullName -notin $before })
        if ($new.Count -ne 1) { throw 'summary missing or ambiguous' }
        $summary = Get-Content -LiteralPath $new[0].FullName -Raw | ConvertFrom-Json
        if (!(Test-Path ($new[0].Directory.FullName + '.zip'))) { throw 'archive missing' }
        if ($summary.control -ne 'fixed-memory-html' -or !$summary.diagnosticOnly -or $summary.latencyComparisonValid) { throw 'diagnostic scope lost' }
        if (($summary.mitigationPolicies | Where-Object scope -eq 'system').status -ne 'unavailable') { throw 'policy query denial lost' }
        if ($summary.processSnapshots.id -contains 999 -or $global:NativeCollectorTest.killed -contains 999) { throw 'unrelated browser touched' }
        if ($mode -eq 'ready' -and !$summary.stableReady) { throw 'ready control not recorded' }
        if ($mode -eq 'timeout' -and !$summary.timedOut) { throw 'timeout not recorded' }
        if ($mode -eq 'exited' -and $summary.hostExitCode -ne 2) { throw 'host exit code lost' }
        if ($mode -eq 'no-log') {
            if ($summary.chromiumLogPresent -or ($summary.processSnapshots | Where-Object kind -eq 'runtime').status -ne 'unavailable') { throw 'missing evidence treated as present' }
        }
        if ($mode -eq 'cleanup-unavailable' -or $mode -eq 'no-log') {
            if ($summary.cleanup.profileRemoved -or !(Test-Path -LiteralPath $summary.cleanup.profile)) { throw 'unconfirmed cleanup deleted profile' }
        } elseif (!$summary.cleanup.profileRemoved) { throw 'profile not cleaned after confirmed exit' }
    }
    Write-Host 'native collector: parsing, fixed one-page scope, logging flags, modules/policy denials, timeout, environment restoration, archives and scoped cleanup passed (mock Windows operations)'
} finally {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $original.args
    $env:TEMP = $original.temp; $env:TMP = $original.tmp; $env:USERPROFILE = $original.user
}
