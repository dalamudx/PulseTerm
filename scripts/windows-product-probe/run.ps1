param([switch]$NativeOnly)
$ErrorActionPreference='Stop'
Write-Host 'windows-product-probe: v1'
$kit=$PSScriptRoot
$tag=[guid]::NewGuid().ToString('N')
$out=Join-Path $kit ('product-results-'+(Get-Date -Format 'yyyyMMdd-HHmmss')+'-'+$tag.Substring(0,6))
$fixture=Join-Path (Join-Path $env:USERPROFILE 'AppData\Local\Temp') ('pulseterm-product-'+$tag)
New-Item -ItemType Directory -Path $out | Out-Null
$original=@{TEMP=$env:TEMP;TMP=$env:TMP;PULSETERM_PRODUCT_PROBE_ROOT=$env:PULSETERM_PRODUCT_PROBE_ROOT}
$created=$false; $removed=$false; $corePassed=$false; $helperLoaded=$false; $nativeCases=@(); $windowRuns=@(); $failure=$null; $build=$null; $role=$null; $nativeReport=$null; $exited=$true; $cleanupErrors=@()
# The helper is checksum-verified before it is executed.
try {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or ![Environment]::Is64BitOperatingSystem) { throw 'Real 64-bit Windows is required.' }
    $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
    try { $role=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) } finally { $identity.Dispose() }
    if ($role -ne $false) { throw 'Use an ordinary, non-elevated PowerShell for this product prerequisite run.' }
    $verified=@{}
    $prefix=[IO.Path]::GetFullPath($kit)+[IO.Path]::DirectorySeparatorChar
    foreach ($line in Get-Content -Encoding UTF8 -LiteralPath (Join-Path $kit 'SHA256SUMS')) {
        if ($line -notmatch '^([a-f0-9]{64})  (.+)$') { throw 'Invalid checksum manifest.' }
        $digest=$Matches[1]; $relative=$Matches[2]
        $file=[IO.Path]::GetFullPath((Join-Path $kit $relative))
        if (!$file.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Checksum path outside kit.' }
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $digest) { throw "Checksum mismatch: $relative" }
        $verified[$relative]=$true
    }
    foreach ($file in @('native-process.ps1','native-tests.json','build-info.json','run.ps1','backend-probe.exe','PulseTerm-Workspace.exe','xtask-tests.exe','pty-tests.exe')) { if (!$verified.ContainsKey($file)) { throw "Missing verified member: $file" } }
    . (Join-Path $kit 'native-process.ps1')
    $helperLoaded=$true
    $manifest=Get-Content -Encoding UTF8 -LiteralPath (Join-Path $kit 'native-tests.json') -Raw | ConvertFrom-Json
    $build=Get-Content -Encoding UTF8 -LiteralPath (Join-Path $kit 'build-info.json') -Raw | ConvertFrom-Json
    $expected=@($manifest.tests)
    if ($manifest.schemaVersion -ne 1 -or $expected.Count -ne 23 -or @($expected.name|Sort-Object -Unique).Count -ne $expected.Count) { throw 'Review native coverage manifest.' }
    New-Item -ItemType Directory -Path $fixture | Out-Null
    $created=$true; $env:TEMP=$fixture; $env:TMP=$fixture; $env:PULSETERM_PRODUCT_PROBE_ROOT=$fixture
    foreach ($binary in @('xtask-tests.exe','pty-tests.exe')) {
        $listStage=($binary -replace '-tests.exe$','')+'-list'
        $code=Invoke-ProductProcess (Join-Path $kit $binary) $listStage @('--list','--format','terse','--color','never') $out
        $names=@(Get-Content -Encoding UTF8 -LiteralPath (Join-Path $out ($listStage+'.log'))|ForEach-Object{if($_ -match '^(.+): test$'){$Matches[1]}})
        if ($code -ne 0 -or !$names.Count) { throw "Native discovery failed: $binary" }
        foreach ($case in @($expected|Where-Object binary -eq $binary)) { if ($case.name -notmatch '^[a-zA-Z0-9_:]+$' -or $case.name -notin $names) { throw "Missing compiled native case: $($case.name)" } }
    }
    $index=0
    foreach ($case in $expected) {
        $index++; $stage='case-'+$index.ToString('000')
        $code=Invoke-ProductProcess (Join-Path $kit $case.binary) $stage @('--exact',$case.name,'--test-threads=1','--format','pretty','--color','never') $out
        $text=Get-Content -Encoding UTF8 -LiteralPath (Join-Path $out ($stage+'.log')) -Raw
        $named=$text -match ('(?m)^test '+[regex]::Escape($case.name)+' \.\.\. ok\r?$')
        $count=$text -match 'test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out;'
        $ok=$code -eq 0 -and $named -and $count
        $nativeCases+=[pscustomobject]@{name=$case.name;binary=$case.binary;stage=$stage;exitCode=$code;passed=$ok}
        if (!$ok) { throw "Native case failed: $($case.name). See $stage.log and $stage-stderr.log." }
    }
    $code=Invoke-ProductProcess (Join-Path $kit 'backend-probe.exe') 'conpty' @('probe-pty-portable') $out
    $report=Join-Path $fixture 'target\probes\pty-portable.json'
    if ($code -ne 0 -or !(Test-Path -LiteralPath $report)) { throw 'Native ConPTY probe failed.' }
    $nativeReport=Get-Content -Encoding UTF8 -LiteralPath $report -Raw|ConvertFrom-Json
    if ($nativeReport.platform -ne 'windows' -or !$nativeReport.nativeTerminal -or !$nativeReport.canonicalInputUnicode -or !$nativeReport.nativeResize -or !$nativeReport.outputEof -or !$nativeReport.directChildTermination -or $nativeReport.exitCode -ne 7) { throw 'Incomplete native ConPTY receipt.' }
    Copy-Item -LiteralPath $report -Destination (Join-Path $out 'conpty.json')
    $corePassed=$true
    if (!$NativeOnly) {
        Write-Host 'Native prerequisites passed. Two real workspace windows will open in sequence.'
        Write-Host 'Window 1: explicitly open a discovered shell; check input, resize, tabs/split; save a setting, then close the app.'
        Write-Host 'Window 2: check persisted settings and Profile; refresh and reopen a shell, then close the app.'
        foreach ($round in 1..2) {
            $code=Invoke-ProductProcess (Join-Path $kit 'PulseTerm-Workspace.exe') ('workspace-'+$round) @() $out 600000
            $windowRuns+=[pscustomobject]@{round=$round;exitCode=$code;manualBehaviorVerified=$false}
            if ($code -ne 0) { throw "Workspace host exited with code $code." }
        }
    }
} catch { $failure=$_ }
finally {
    if ($helperLoaded) {
        try { $exited=Stop-ProductProcess } catch { $exited=$false; $cleanupErrors+=$_.FullyQualifiedErrorId }
    }
    foreach ($name in $original.Keys) { if ($null -eq $original[$name]) { Remove-Item ('Env:'+ $name) -ErrorAction SilentlyContinue } else { Set-Item ('Env:'+ $name) $original[$name] } }
    if ($created -and $exited) {
        try { Remove-Item -LiteralPath $fixture -Recurse -Force } catch { $cleanupErrors+=$_.FullyQualifiedErrorId }
        $removed=!(Test-Path -LiteralPath $fixture)
    }
    $corePassed=$corePassed -and $exited -and $created -and $removed -and !$cleanupErrors.Count
    $runs=@(); if (Get-Variable ProductRuns -Scope Script -ErrorAction SilentlyContinue) { $runs=@($script:ProductRuns) }
    $summary=[pscustomobject]@{schemaVersion=1;scope='Selected Windows production storage/Profile prerequisites and direct-child ConPTY; GUI behavior requires manual evidence';build=$build;windowsVersion=[Environment]::OSVersion.VersionString;elevatedAdministrator=$role;nativeCorePassed=$corePassed;nativeCasesExpected=23;nativeCases=$nativeCases;conpty=$nativeReport;workspaceRuns=$windowRuns;productionStartupValidated=$false;ordinaryUserProductAcceptance='pending';latencyComparisonValid=$false;error=if($failure){$failure.Exception.Message}else{$null};processRuns=$runs;cleanup=[pscustomobject]@{processExited=$exited;fixtureCreated=$created;fixtureRemoved=$removed;fixture=$fixture;errors=$cleanupErrors}}
    [IO.File]::WriteAllText((Join-Path $out 'product-summary.json'),($summary|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    Compress-Archive -Path "$out\*" -DestinationPath "$out.zip" -Force
    Write-Host "Send this product prerequisite archive back: $out.zip"
    if ($created -and !$removed) { Write-Warning "Fixture retained: $fixture" }
}
if ($failure) { throw $failure }
if (!$corePassed) { throw 'Native prerequisites or cleanup were not confirmed.' }
