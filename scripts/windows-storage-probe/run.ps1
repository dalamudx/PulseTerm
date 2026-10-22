# Windows PowerShell 5.1; existing platform tests, no production startup or policy changes.
$ErrorActionPreference = 'Stop'
Write-Host 'windows-storage-probe: v3'
if ($env:OS -ne 'Windows_NT' -or ![Environment]::Is64BitOperatingSystem) { throw 'Run on real 64-bit Windows. Wine is not native acceptance.' }
$kit = $PSScriptRoot
$tag = [guid]::NewGuid().ToString('N')
$out = Join-Path $kit ('storage-results-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + $tag.Substring(0, 6))
$tempRoot = Join-Path (Join-Path $env:USERPROFILE 'AppData\Local\Temp') ('pulseterm-storage-' + $tag)
New-Item -ItemType Directory -Path $out | Out-Null
$oldTemp = $env:TEMP; $oldTmp = $env:TMP
$tempCreated = $false; $tempRemoved = $false; $failure = $null
$script:storageProcess = $null
$discovered = @(); $native = @(); $nativeResults = @(); $runExit = $null; $counts = $null; $passed = $false
$nativeExpected = @(); $build = $null; $script:storageTimedOut = $false
$listExit = $null; $script:storageRuns = @(); $script:storageActiveRun = $null; $script:storageOutput = @()

$privilegeContext = [pscustomobject]@{ status='unavailable'; elevatedAdministrator=$null }

function Complete-StorageOutput {
    foreach ($output in $script:storageOutput) {
        if (!$output.task.Wait(5000)) { throw 'Native output did not finish after process exit.' }
        [IO.File]::WriteAllText($output.path, $output.task.GetAwaiter().GetResult(), [Text.UTF8Encoding]::new($false))
    }
    $script:storageOutput = @()
}

function Invoke-StorageProcess([string]$Stage, [string[]]$Arguments) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = Join-Path $kit 'platform-tests.exe'
    # Every argument is a fixed flag/value selected below, never caller text.
    $info.Arguments = $Arguments -join ' '
    $info.UseShellExecute = $false; $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true

    $info.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $info.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    # Own the process handle from Start, including children that exit immediately.
    $script:storageProcess = [Diagnostics.Process]::new()
    $script:storageProcess.StartInfo = $info
    $run = [pscustomobject]@{ stage=$Stage; id=$null; started=$false; exited=$false; exitCode=$null; exitCodeAvailable=$false; timedOut=$false }
    $script:storageRuns += $run; $script:storageActiveRun = $run
    if (!$script:storageProcess.Start()) { throw "Native $Stage process did not start." }
    $run.started = $true; $run.id = $script:storageProcess.Id
    $script:storageOutput = @(
        [pscustomobject]@{ path=(Join-Path $out ($Stage + '.log')); task=$script:storageProcess.StandardOutput.ReadToEndAsync() },
        [pscustomobject]@{ path=(Join-Path $out ($Stage + '-stderr.log')); task=$script:storageProcess.StandardError.ReadToEndAsync() }
    )
    if (!$script:storageProcess.WaitForExit(120000)) {
        $run.timedOut = $true; $script:storageTimedOut = $true
        throw 'Native test process timed out.'
    }
    $script:storageProcess.WaitForExit()
    $run.exited = $script:storageProcess.HasExited
    Complete-StorageOutput
    $code = $script:storageProcess.ExitCode
    $run.exitCode = $code; $run.exitCodeAvailable = $code -is [int]
    if (!$run.exited -or !$run.exitCodeAvailable) { throw "Native $Stage exit code unavailable after wait." }
    $script:storageProcess.Dispose(); $script:storageProcess = $null; $script:storageActiveRun = $null
    return $code
}

try {
    $identity = $null
    try {
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = [Security.Principal.WindowsPrincipal]::new($identity)
        $privilegeContext.elevatedAdministrator = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
        $privilegeContext.status = 'read'
    } catch { $privilegeContext.status = 'unavailable' }
    finally { if ($null -ne $identity) { $identity.Dispose() } }
    $verified = @{}
    $kitPrefix = [IO.Path]::GetFullPath($kit) + [IO.Path]::DirectorySeparatorChar
    foreach ($line in Get-Content -Encoding UTF8 -LiteralPath (Join-Path $kit 'SHA256SUMS')) {
        if ($line -notmatch '^([a-f0-9]{64})  (.+)$') { throw 'Invalid checksum manifest.' }
        $expected = $Matches[1]; $relative = $Matches[2]
        $file = [IO.Path]::GetFullPath((Join-Path $kit $relative))
        if (!$file.StartsWith($kitPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Checksum path outside kit.' }
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $expected) { throw "Checksum mismatch: $relative" }
        $verified[$relative] = $true
    }
    foreach ($required in @('platform-tests.exe', 'native-tests.json', 'build-info.json', 'run.ps1')) {
        if (!$verified.ContainsKey($required)) { throw "Missing verified kit member: $required" }
    }
    $manifest = Get-Content -Encoding UTF8 -LiteralPath (Join-Path $kit 'native-tests.json') -Raw | ConvertFrom-Json
    $build = Get-Content -Encoding UTF8 -LiteralPath (Join-Path $kit 'build-info.json') -Raw | ConvertFrom-Json
    $nativeExpected = @($manifest.nativeTests)
    if ($manifest.schemaVersion -ne 1 -or $manifest.target -ne 'x86_64-pc-windows-gnu' -or !$nativeExpected.Count -or @($nativeExpected | Sort-Object -Unique).Count -ne $nativeExpected.Count) { throw 'Missing or invalid native test coverage.' }
    New-Item -ItemType Directory -Path $tempRoot | Out-Null
    $tempCreated = $true
    $env:TEMP = $tempRoot; $env:TMP = $tempRoot
    $listExit = Invoke-StorageProcess 'list' @('--list', '--format', 'terse', '--color', 'never')
    $discovered = @(Get-Content -Encoding UTF8 -LiteralPath (Join-Path $out 'list.log') | ForEach-Object { if ($_ -match '^(.+): test$') { $Matches[1] } })
    $native = @($discovered | Where-Object { $_.StartsWith('windows::', [StringComparison]::Ordinal) })
    if ($listExit -ne 0) { throw "Native test discovery failed with exit code $listExit. See list-stderr.log." }
    if (!$discovered.Count -or !$native.Count -or (Compare-Object ($nativeExpected | Sort-Object) ($native | Sort-Object))) { throw 'Native test list does not match the compiled source manifest.' }
    $runExit = Invoke-StorageProcess 'tests' @('--test-threads=1', '--format', 'pretty', '--color', 'never')
    $lines = @(Get-Content -Encoding UTF8 -LiteralPath (Join-Path $out 'tests.log'))
    $nativeResults = @($lines | ForEach-Object {
        if ($_ -match '^test (windows::\S+) \.\.\. (ok|FAILED|ignored)$') { [pscustomobject]@{ name=$Matches[1]; status=$Matches[2] } }
    })
    foreach ($line in $lines) {
        if ($line -match '^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;') {
            $counts = [pscustomobject]@{ status=$Matches[1]; passed=[int]$Matches[2]; failed=[int]$Matches[3]; ignored=[int]$Matches[4]; measured=[int]$Matches[5]; filtered=[int]$Matches[6] }
        }
    }
    $passed = $runExit -eq 0 -and $null -ne $counts -and $counts.status -eq 'ok' -and $counts.passed -eq $discovered.Count -and $counts.failed -eq 0 -and $counts.ignored -eq 0 -and $counts.measured -eq 0 -and $counts.filtered -eq 0 -and $nativeResults.Count -eq $nativeExpected.Count -and @($nativeResults | Where-Object status -ne 'ok').Count -eq 0 -and !(Compare-Object ($nativeExpected | Sort-Object) ($nativeResults.name | Sort-Object))
    if (!$passed) { throw 'Native storage tests did not all pass. See tests.log and tests-stderr.log.' }
} catch { $failure = $_ }
finally {
    $env:TEMP = $oldTemp; $env:TMP = $oldTmp
    $exited = $true; $cleanupErrors = @()
    if ($script:storageProcess) {
        try {
            if ($script:storageActiveRun.started) {
                if (!$script:storageProcess.HasExited) {
                    $script:storageProcess.Kill()
                    if (!$script:storageProcess.WaitForExit(5000)) { $exited = $false }
                }
                $script:storageActiveRun.exited = $exited
                if ($exited) {
                    $code = $script:storageProcess.ExitCode
                    $script:storageActiveRun.exitCode = $code
                    $script:storageActiveRun.exitCodeAvailable = $code -is [int]
                }
            }
        } catch { $exited = $false; $cleanupErrors += $_.FullyQualifiedErrorId }
        if ($exited) {
            try { Complete-StorageOutput } catch { $cleanupErrors += $_.FullyQualifiedErrorId }
        }
        $script:storageProcess.Dispose(); $script:storageProcess = $null
    }
    # Preserve successful discovery output even when its exit code is missing/nonzero.
    if (Test-Path -LiteralPath (Join-Path $out 'list.log')) {
        $discovered = @(Get-Content -Encoding UTF8 -LiteralPath (Join-Path $out 'list.log') | ForEach-Object { if ($_ -match '^(.+): test$') { $Matches[1] } })
        $native = @($discovered | Where-Object { $_.StartsWith('windows::', [StringComparison]::Ordinal) })
    }
    if ($tempCreated -and $exited) {
        try { Remove-Item -LiteralPath $tempRoot -Recurse -Force } catch { $cleanupErrors += $_.FullyQualifiedErrorId }
        $tempRemoved = !(Test-Path -LiteralPath $tempRoot)
    }
    if ($tempCreated -and !$tempRemoved) { Write-Warning "Fixture retained: $tempRoot" }
    $passed = $passed -and $exited -and $tempCreated -and $tempRemoved
    [pscustomobject]@{ schemaVersion=1; diagnosticOnly=$true; productionStartupValidated=$false; latencyComparisonValid=$false;
        windowsVersion=[Environment]::OSVersion.VersionString; privilegeContext=$privilegeContext; build=$build; passed=$passed; error=if ($failure) { $failure.Exception.Message } else { $null };
        nativeTestsExpected=$nativeExpected; nativeTestsDiscovered=$native; discoveredCount=$discovered.Count; nativeResults=$nativeResults;
        discoveryExitCode=$listExit; runExitCode=$runExit; processRuns=$script:storageRuns; timedOut=$script:storageTimedOut; resultCounts=$counts;
        cleanup=[pscustomobject]@{ processExited=$exited; fixtureCreated=$tempCreated; fixtureRemoved=$tempRemoved; fixture=$tempRoot; errors=$cleanupErrors } } |
        ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $out 'storage-summary.json')
    Compress-Archive -Path "$out\*" -DestinationPath "$out.zip" -Force
    Write-Host "Send this storage prerequisite archive back: $out.zip"
}
if ($failure) { throw $failure }

if (!$passed) { throw 'Storage prerequisite cleanup was not confirmed. See the diagnostic archive.' }
