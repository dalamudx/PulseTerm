# One fixed in-memory page using the existing v6 host. Windows PowerShell 5.1.
param([string]$RuntimePath)
$ErrorActionPreference = 'Stop'
Write-Host 'echo-native-collector: v1'
$kit = $PSScriptRoot
$hostExe = Join-Path $kit 'echo-webview2.exe'
foreach ($file in @($hostExe, (Join-Path $kit 'WebView2Loader.dll'))) {
    if (!(Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing $file. Put this script in the complete v6 kit directory." }
}
if ($RuntimePath) {
    $RuntimePath = (Get-Item -LiteralPath $RuntimePath).FullName
    if (!(Test-Path -LiteralPath (Join-Path $RuntimePath 'msedgewebview2.exe') -PathType Leaf)) { throw 'Invalid RuntimePath.' }
}
$tag = [guid]::NewGuid().ToString('N')
$out = Join-Path $kit ('native-startup-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + $tag.Substring(0, 6))
$profile = Join-Path (Join-Path $env:USERPROFILE 'AppData\Local\Temp') ('pulseterm-native-' + $tag)
New-Item -ItemType Directory -Path $out | Out-Null
New-Item -ItemType Directory -Path $profile | Out-Null
$browserLog = Join-Path $out 'chromium.log'
$hostLog = Join-Path $out 'host.log'
$oldArgs = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$oldTemp = $env:TEMP; $oldTmp = $env:TMP
$hostProcess = $null; $hostExit = $null; $failure = $null
$snapshots = @(); $seen = @{}; $issues = @(); $policies = @()
$observedReady = $false; $stableReady = $false; $timedOut = $false
$childrenConfirmedGone = $false; $hostConfirmedGone = $false

function Read-ModuleSnapshot([int]$ProcessId, [string]$Kind) {
    $process = $null
    try {
        $process = Get-Process -Id $ProcessId -ErrorAction Stop
        $all = @($process.Modules)
        $modules = @($all | Select-Object -First 256 | ForEach-Object {
            [pscustomobject]@{ name=$_.ModuleName; path=$_.FileName; company=$_.FileVersionInfo.CompanyName; version=$_.FileVersionInfo.FileVersion }
        })
        [pscustomobject]@{ id=$ProcessId; kind=$Kind; status='read'; modules=$modules; truncated=($all.Count -gt 256); error=$null }
    } catch {
        [pscustomobject]@{ id=$ProcessId; kind=$Kind; status='unavailable'; modules=@(); truncated=$false; error=$_.FullyQualifiedErrorId }
    } finally { if ($process) { $process.Dispose() } }
}

try {
    foreach ($scope in @('system', 'echo-webview2.exe', 'msedgewebview2.exe')) {
        try {
            $policy = if ($scope -eq 'system') { Get-ProcessMitigation -System -ErrorAction Stop } else { Get-ProcessMitigation -Name $scope -ErrorAction Stop }
            $text = $policy | Out-String -Width 200
            $policies += [pscustomobject]@{ scope=$scope; status='read'; text=$text.Substring(0, [math]::Min(65536, $text.Length)); error=$null }
        } catch { $policies += [pscustomobject]@{ scope=$scope; status='unavailable'; text=$null; error=$_.FullyQualifiedErrorId } }
    }
    # Logging only: no remote debugging port, sandbox/GPU changes, or inherited extra flags.
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--enable-logging --v=1 --log-file=`"$browserLog`""
    $env:TEMP = Split-Path $profile -Parent; $env:TMP = $env:TEMP
    $arguments = @('--url', 'about:blank', '--startup-memory', '1', '--profile', "`"$profile`"", '--log', "`"$hostLog`"")
    if ($RuntimePath) { $arguments += @('--runtime', "`"$RuntimePath`"") }
    $hostProcess = Start-Process -FilePath $hostExe -ArgumentList $arguments -PassThru
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $oldArgs
    $env:TEMP = $oldTemp; $env:TMP = $oldTmp
    $snapshots += Read-ModuleSnapshot $hostProcess.Id 'host'
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    $readyAt = $null; $cimFailed = $false
    do {
        if (!$cimFailed) {
            try {
                $browsers = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" -ErrorAction Stop |
                    Where-Object { $_.CommandLine -and $_.CommandLine.IndexOf($profile, [StringComparison]::OrdinalIgnoreCase) -ge 0 })
                foreach ($browser in $browsers) {
                    if ($seen.Count -ge 32) { break }
                    if (!$seen.ContainsKey([int]$browser.ProcessId)) {
                        $seen[[int]$browser.ProcessId] = $true
                        $snapshots += Read-ModuleSnapshot $browser.ProcessId 'runtime'
                    }
                }
            } catch { $cimFailed = $true; $issues += 'process discovery unavailable: ' + $_.FullyQualifiedErrorId }
        }
        $hostProcess.Refresh()
        if ($hostProcess.HasExited) { $hostExit = $hostProcess.ExitCode; break }
        if ($hostProcess.MainWindowTitle -eq 'echo-startup-ready') {
            $observedReady = $true
            if (!$readyAt) { $readyAt = [DateTime]::UtcNow }
            if (([DateTime]::UtcNow - $readyAt).TotalSeconds -ge 3) { $stableReady = $true; break }
        } else { $readyAt = $null }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    $timedOut = !$hostProcess.HasExited -and !$stableReady
} catch { $failure = $_; $issues += 'collection failed: ' + $_.FullyQualifiedErrorId }
finally {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $oldArgs
    $env:TEMP = $oldTemp; $env:TMP = $oldTmp
    try {
        if ($hostProcess) {
            $hostProcess.Refresh()
            if (!$hostProcess.HasExited) {
                $null = $hostProcess.CloseMainWindow()
                if (!$hostProcess.WaitForExit(5000)) { $hostProcess.Kill(); $null = $hostProcess.WaitForExit(5000) }
            }
            $hostConfirmedGone = $hostProcess.HasExited
            if ($hostConfirmedGone -and $null -eq $hostExit) { $hostExit = $hostProcess.ExitCode }
        } else { $hostConfirmedGone = $true }
    } catch { $issues += 'host cleanup unavailable: ' + $_.FullyQualifiedErrorId }
    try {
        $remaining = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" -ErrorAction Stop |
            Where-Object { $_.CommandLine -and $_.CommandLine.IndexOf($profile, [StringComparison]::OrdinalIgnoreCase) -ge 0 })
        $childrenConfirmedGone = $true
        foreach ($child in $remaining) {
            $process = Get-Process -Id $child.ProcessId -ErrorAction SilentlyContinue
            if ($process) {
                try {
                    if (!$process.HasExited) {
                        $process.Kill()
                        if (!$process.WaitForExit(5000)) { $childrenConfirmedGone = $false }
                    }
                } finally { $process.Dispose() }
            }
        }
    } catch { $childrenConfirmedGone = $false; $issues += 'runtime cleanup unavailable: ' + $_.FullyQualifiedErrorId }
    if ($hostConfirmedGone -and $childrenConfirmedGone) {
        Remove-Item -LiteralPath $profile -Recurse -Force -ErrorAction SilentlyContinue
    }
    if ($hostProcess) { $hostProcess.Dispose() }
    $profileRemoved = !(Test-Path -LiteralPath $profile)
    if (!$profileRemoved) { Write-Warning "Profile retained because cleanup was not confirmed: $profile" }
    $logPresent = Test-Path -LiteralPath $browserLog -PathType Leaf
    [pscustomobject]@{ schemaVersion=1; diagnosticOnly=$true; latencyComparisonValid=$false; control='fixed-memory-html';
        runtimeRequested=$RuntimePath; inheritedExtraArgumentsPresent=![string]::IsNullOrEmpty($oldArgs); chromiumLogPresent=$logPresent;
        observedReady=$observedReady; stableReady=$stableReady; timedOut=$timedOut; hostExitCode=$hostExit;
        processSnapshots=$snapshots; mitigationPolicies=$policies; issues=$issues;
        cleanup=[pscustomobject]@{ hostExited=$hostConfirmedGone; runtimeExited=$childrenConfirmedGone; profileRemoved=$profileRemoved; profile=$profile } } |
        ConvertTo-Json -Depth 7 | Set-Content -Encoding UTF8 (Join-Path $out 'native-summary.json')
    Compress-Archive -Path "$out\*" -DestinationPath "$out.zip" -Force
    Write-Host "Send this native startup archive back: $out.zip"
    if (!$logPresent) { Write-Warning 'Runtime did not create chromium.log; absence of a log does not establish the cause.' }
}
if ($failure) { throw $failure }
