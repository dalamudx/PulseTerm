param([Parameter(Mandatory=$true)][string]$TestRoot, [string]$NativeFixture)
$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot 'run.ps1'
$tokens=$null; $errors=$null
$null = [System.Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
if ([IO.File]::ReadAllBytes($source) | Where-Object { $_ -gt 127 }) { throw 'runner must remain ASCII for PS5.1' }
New-Item -ItemType Directory -Path $TestRoot | Out-Null
$kit = Join-Path $TestRoot 'kit with spaces'
New-Item -ItemType Directory -Path $kit | Out-Null
$text = [IO.File]::ReadAllText($source)
if (($text.Split(@('[Diagnostics.Process]::new()'), [StringSplitOptions]::None)).Count -ne 2) { throw 'process construction seam changed' }
# Replace only process construction; waits, output tasks and exit handling remain real code.
[IO.File]::WriteAllText((Join-Path $kit 'run.ps1'), $text.Replace('[Diagnostics.Process]::new()', '(New-StorageFixtureProcess)'))
$nativeNames = @('windows::tests::first', 'windows::private_file::atomic_file::tests::second')
$names = @($nativeNames) + @('policy::tests::third')
[pscustomobject]@{schemaVersion=1;target='x86_64-pc-windows-gnu';nativeTests=$nativeNames} | ConvertTo-Json | Set-Content (Join-Path $kit 'native-tests.json')
'{}' | Set-Content (Join-Path $kit 'build-info.json')
$original = @{ os=$env:OS; user=$env:USERPROFILE; temp=$env:TEMP; tmp=$env:TMP; fixtureMode=$env:PULSE_STORAGE_FIXTURE_MODE }
$env:OS = 'Windows_NT'; $env:USERPROFILE = Join-Path $TestRoot 'user'
$env:TEMP='fixture-original-temp'; $env:TMP='fixture-original-tmp'
function New-StorageFixtureProcess {
    if ($global:StorageRunnerFixture.real) { return [Diagnostics.Process]::new() }
    $process = [pscustomobject]@{ StartInfo=$null; HasExited=$false; ExitCode=$null; Id=42; Killed=$false; StandardOutput=$null; StandardError=$null }
    $process | Add-Member -MemberType ScriptMethod -Name Start -Value {
        $global:StorageRunnerFixture.launches++
        if (!(Split-Path $this.StartInfo.FileName -Leaf).Equals('platform-tests.exe')) { throw 'unexpected executable' }
        if ($this.StartInfo.UseShellExecute -or !$this.StartInfo.RedirectStandardOutput -or !$this.StartInfo.RedirectStandardError) { throw 'owned redirected process required' }

        if ($this.StartInfo.StandardOutputEncoding.CodePage -ne 65001 -or $this.StartInfo.StandardErrorEncoding.CodePage -ne 65001) { throw 'native logs must decode UTF-8' }
        if ($env:TEMP -eq 'fixture-original-temp' -or $env:TEMP -ne $env:TMP) { throw 'fixture isolation missing' }
        $mode=$global:StorageRunnerFixture.mode
        if ($mode -eq 'launch-failure') { throw 'fixture launch denied' }
        $list = $this.StartInfo.Arguments -eq '--list --format terse --color never'
        if (!$list -and $this.StartInfo.Arguments -ne '--test-threads=1 --format pretty --color never') { throw 'unexpected test filters' }
        if ($list) {
            $selected = if ($mode -eq 'native-missing') { @('policy::tests::third') } else { $names }
            $stdout = ($selected | ForEach-Object { $_ + ': test' }) -join "`n"
        } else {
            $statuses = @('ok', 'ok', 'ok')
            if ($mode -eq 'failed') { $statuses[0]='FAILED' }
            if ($mode -eq 'ignored') { $statuses[0]='ignored' }
            $lines = @(for ($i=0; $i -lt $names.Count; $i++) { 'test ' + $names[$i] + ' ... ' + $statuses[$i] })
            $summary = switch ($mode) {
                'failed' { 'test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;' }
                'ignored' { 'test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;' }
                'zero-tests' { 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;' }
                default { 'test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;' }
            }
            $stdout = ($lines + @($summary)) -join "`n"
        }
        $this.StandardOutput = [IO.StringReader]::new([string]$stdout)
        $this.StandardError = [IO.StringReader]::new('fixture stderr')
        $this.HasExited = $mode -notin @('timeout','stop-failed')
        $this.ExitCode = if ($mode -eq 'missing-exit') {$null} elseif ($list -and $mode -eq 'list-failed') {37} elseif (!$list -and $mode -eq 'failed') {101} else {0}
        return $true
    }
    $process | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {
        param($Milliseconds)
        if ($null -eq $Milliseconds) { return }
        if ($global:StorageRunnerFixture.mode -in @('timeout','stop-failed') -and !$this.Killed) { return $false }
        return $true
    }
    $process | Add-Member -MemberType ScriptMethod -Name Kill -Value {
        if ($global:StorageRunnerFixture.mode -eq 'stop-failed') { throw 'fixture stop failed' }
        $this.Killed=$true; $this.HasExited=$true
    }
    $process | Add-Member -MemberType ScriptMethod -Name Dispose -Value { }
    $process
}
function Remove-Item {
    param($LiteralPath, [switch]$Recurse, [switch]$Force)
    if ($global:StorageRunnerFixture.mode -eq 'cleanup-failed') { throw 'fixture cleanup denied' }
    Microsoft.PowerShell.Management\Remove-Item -LiteralPath $LiteralPath -Recurse:$Recurse -Force:$Force
}
try {
    $cases = @('passed','failed','zero-tests','native-missing','ignored','list-failed','missing-exit','timeout','stop-failed','launch-failure','checksum-failed','cleanup-failed') | ForEach-Object { [pscustomobject]@{mode=$_;real=$false} }
    if ($NativeFixture) { $cases += @('passed','passed','passed','passed','passed','passed','passed','passed','failed','list-failed') | ForEach-Object { [pscustomobject]@{mode=$_;real=$true} } }
    foreach ($case in $cases) {
        $mode=$case.mode
        $global:StorageRunnerFixture = [pscustomobject]@{mode=$mode;launches=0;real=$case.real}
        $env:PULSE_STORAGE_FIXTURE_MODE=$mode
        if ($case.real) { Copy-Item -LiteralPath $NativeFixture -Destination (Join-Path $kit 'platform-tests.exe') -Force } else { 'fixture binary' | Set-Content (Join-Path $kit 'platform-tests.exe') }
        @('run.ps1','platform-tests.exe','native-tests.json','build-info.json') | ForEach-Object { (Get-FileHash -LiteralPath (Join-Path $kit $_) -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + $_ } | Set-Content (Join-Path $kit 'SHA256SUMS')
        if ($mode -eq 'checksum-failed') { 'modified' | Set-Content (Join-Path $kit 'platform-tests.exe') }
        $before = @(Get-ChildItem $kit -Filter storage-summary.json -Recurse | ForEach-Object FullName)
        $threw=$false
        try { & (Join-Path $kit 'run.ps1') } catch { $threw=$true }
        if ($threw -ne ($mode -ne 'passed')) { throw "unexpected runner completion: $mode / real=$($case.real)" }
        if ($env:TEMP -ne 'fixture-original-temp' -or $env:TMP -ne 'fixture-original-tmp') { throw 'environment not restored' }
        $result = @(Get-ChildItem $kit -Filter storage-summary.json -Recurse | Where-Object { $_.FullName -notin $before })
        if ($result.Count -ne 1 -or !(Test-Path ($result[0].Directory.FullName + '.zip'))) { throw 'missing failure archive' }
        $summary = Get-Content -LiteralPath $result[0].FullName -Raw | ConvertFrom-Json
        if ($summary.passed -ne ($mode -eq 'passed')) { throw "false acceptance: $mode" }
        if (!$summary.diagnosticOnly -or $summary.productionStartupValidated -or $summary.latencyComparisonValid) { throw 'scope widened' }
        if ($mode -eq 'checksum-failed' -and $global:StorageRunnerFixture.launches) { throw 'unverified binary launched' }
        if ($mode -in @('cleanup-failed','stop-failed')) {
            if ($summary.cleanup.fixtureRemoved -or !(Test-Path $summary.cleanup.fixture)) { throw 'unconfirmed cleanup hidden' }
        } elseif ($summary.cleanup.fixtureCreated -and !$summary.cleanup.fixtureRemoved) { throw 'confirmed fixture not cleaned' }
        if ($mode -eq 'timeout' -and !$summary.timedOut) { throw 'timeout evidence lost' }
        if ($mode -eq 'passed' -and ($summary.discoveryExitCode -ne 0 -or $summary.runExitCode -ne 0 -or $summary.processRuns.Count -ne 2)) { throw 'scalar zero exit code or stage evidence lost' }

        if ($mode -eq 'failed' -and ($summary.discoveryExitCode -ne 0 -or $summary.runExitCode -ne 101 -or $summary.processRuns[1].exitCode -ne 101)) { throw 'nonzero execution evidence lost' }
        if ($mode -eq 'passed') {
            $stderr = [IO.File]::ReadAllText((Join-Path $result[0].Directory.FullName 'tests-stderr.log'))
            if ($stderr -notmatch 'fixture stderr') { throw 'redirected stderr lost' }

            if ($case.real -and !$stderr.Contains([string][char]0x4e2d)) { throw 'native UTF-8 stderr corrupted' }
            if ($summary.privilegeContext.status -notin @('read','unavailable')) { throw 'privilege context missing' }
        }
        if ($mode -eq 'list-failed' -and ($summary.discoveryExitCode -ne 37 -or $summary.discoveredCount -ne 3)) { throw 'nonzero discovery evidence lost' }
        if ($mode -eq 'missing-exit' -and ($summary.processRuns[0].exitCodeAvailable -or $summary.discoveredCount -ne 3 -or $summary.error -notmatch 'exit code unavailable')) { throw 'unknown exit silently accepted or discovery output lost' }
    }
    Write-Host "storage runner: 12 mocked scenarios and $(@($cases | Where-Object real).Count) real fast-process scenarios passed; explicit exit codes, retained discovery, zero/ignored rejection and cleanup"
} finally {
    $env:OS=$original.os; $env:USERPROFILE=$original.user; $env:TEMP=$original.temp; $env:TMP=$original.tmp; $env:PULSE_STORAGE_FIXTURE_MODE=$original.fixtureMode
}
