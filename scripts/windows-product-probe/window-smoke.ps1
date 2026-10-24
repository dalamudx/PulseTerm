# Real production window smoke on disposable Windows CI; never ordinary-user acceptance.
param([string]$Executable = 'target/debug/pulseterm-workspace.exe')
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or $env:GITHUB_ACTIONS -ne 'true') {
    throw 'Requires a disposable Windows GitHub runner.'
}
$runId = [Guid]::NewGuid().ToString('N')
$out = Join-Path ([IO.Path]::GetFullPath('target/probes/windows-product-window')) $runId
New-Item -ItemType Directory -Path $out | Out-Null
$exe = (Resolve-Path -LiteralPath $Executable).Path
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
try { $elevated = ([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) }
finally { $identity.Dispose() }
$dataRoots = @(
    (Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'dev.pulseterm.workspace-preview'),
    (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'dev.pulseterm.workspace-preview')
) | Select-Object -Unique
foreach ($path in $dataRoots) {
    if (Test-Path -LiteralPath $path) { throw 'Refusing pre-existing product data; use a fresh runner.' }
}
$tempRoot = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) ('PulseTerm-window-' + $runId)
New-Item -ItemType Directory -Path $tempRoot | Out-Null
$original = @{ TMPDIR = $env:TMPDIR; TMP = $env:TMP; TEMP = $env:TEMP }
$env:TMPDIR = $tempRoot; $env:TMP = $tempRoot; $env:TEMP = $tempRoot
$app = $null; $started = $false; $stdout = $null; $stderr = $null; $failure = $null
$ready = $false; $normalExit = $false; $drained = $false; $forced = $false
$children = @{}; $cleanupErrors = @(); $visibleProfiles = @()
function Remember-Children {
    if (!$app) { return }
    $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId)
    $parents = @{}; $parents[$app.Id] = $true
    foreach ($id in $children.Keys) { $parents[$id] = $true }
    do {
        $added = $false
        foreach ($row in $all) {
            $childId = [int]$row.ProcessId
            if ($parents.ContainsKey([int]$row.ParentProcessId) -and !$parents.ContainsKey($childId)) {
                $child = $null
                try {
                    $child = [Diagnostics.Process]::GetProcessById($childId)
                    # Open and retain the exact process handle, including during failure cleanup.
                    $null = $child.Handle
                    if ($child.StartTime.ToUniversalTime() -lt $app.StartTime.ToUniversalTime()) { $child.Dispose(); continue }
                    $children[$childId] = $child; $parents[$childId] = $true; $added = $true
                } catch [ArgumentException] { }
                catch [InvalidOperationException] {
                    if ($child -and $child.HasExited) { $child.Dispose() } else { throw }
                }
            }
        }
    } while ($added)
}
try {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    . (Join-Path $PSScriptRoot 'current-user-owner.ps1')
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $exe; $info.UseShellExecute = $false; $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
    $app = [Diagnostics.Process]::new(); $app.StartInfo = $info
    if (!$app.Start()) { throw 'Product process did not start.' }
    $started = $true
    $stdout = $app.StandardOutput.ReadToEndAsync(); $stderr = $app.StandardError.ReadToEndAsync()
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    do {
        if ($app.HasExited) { throw 'Product exited before the workspace was ready.' }
        Remember-Children
        $app.Refresh()
        if ($app.MainWindowHandle -ne [IntPtr]::Zero) {
            $window = [Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
            if ($window.Current.ProcessId -ne $app.Id) { throw 'Window process identity mismatch.' }
            $buttons = $window.FindAll([Windows.Automation.TreeScope]::Descendants,
                [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Button))
            $visibleProfiles = @($buttons | Where-Object { $_.Current.IsEnabled -and !$_.Current.IsOffscreen -and $_.Current.Name -in @('Windows PowerShell', 'PowerShell (pwsh)', 'Command Prompt') } | ForEach-Object { $_.Current.Name })
            $ready = $visibleProfiles.Count -gt 0
        }
        if (!$ready) { Start-Sleep -Milliseconds 250 }
    } while (!$ready -and [DateTime]::UtcNow -lt $deadline)
    if (!$ready) { throw 'No visible enabled backend-discovered Profile button within 60 seconds.' }
    Remember-Children
    if (!$app.CloseMainWindow()) { throw 'Could not request native window close.' }
    if (!$app.WaitForExit(30000)) { throw 'Product did not exit within 30 seconds after close.' }
    $normalExit = $app.ExitCode -eq 0
    if (!$normalExit) { throw 'Product returned a nonzero exit code.' }
    foreach ($child in $children.Values) {
        if (!$child.WaitForExit(10000)) { throw 'An observed WebView descendant remained after product exit.' }
    }
    if (!$stderr.Wait(5000) -or !$stdout.Wait(5000)) { throw 'Output capture did not finish.' }
    $lines = $stderr.GetAwaiter().GetResult() -split '\r?\n'
    foreach ($marker in @('workspace.document_confirmed', 'workspace.profiles_listed', 'workspace.native_drained', 'workspace.profiles_drained', 'workspace.shutdown_complete')) {
        if ($lines -notcontains $marker) { throw "Required native lifecycle marker missing: $marker" }
    }
    $drained = $true
} catch {
    $failure = $_.Exception.Message
    # Diagnostic only: capture this empty product window, without dumping arbitrary UI text.
    try {
        if ($window) {
            Add-Type -AssemblyName System.Drawing
            $bounds = $window.Current.BoundingRectangle
            $bitmap = [Drawing.Bitmap]::new([int]$bounds.Width, [int]$bounds.Height)
            $graphics = [Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.CopyFromScreen([int]$bounds.X, [int]$bounds.Y, 0, 0, $bitmap.Size)
                $bitmap.Save((Join-Path $out 'failure.png'))
            } finally { $graphics.Dispose(); $bitmap.Dispose() }
        }
    } catch { Write-Warning 'Window screenshot unavailable.' }
}
finally {
    if ($app -and $started) {
        try { Remember-Children } catch { $cleanupErrors += $_.Exception.Message }
        try {
            if (!$app.HasExited) { $forced = $true; $app.Kill() }
            if (!$app.WaitForExit(5000)) { throw 'Product cleanup did not complete.' }
        } catch { $cleanupErrors += $_.Exception.Message }
        foreach ($child in $children.Values) {
            try {
                if (!$child.HasExited) { $forced = $true; $child.Kill() }
                if (!$child.WaitForExit(5000)) { throw "Observed child cleanup did not complete: $($child.Id)" }
            } catch { $cleanupErrors += $_.Exception.Message }
            finally { $child.Dispose() }
        }
        foreach ($capture in @(@{task=$stdout; name='stdout.log'}, @{task=$stderr; name='stderr.log'})) {
            try {
                if ($capture.task) {
                    if (!$capture.task.Wait(5000)) { throw "Capture incomplete: $($capture.name)" }
                    [IO.File]::WriteAllText((Join-Path $out $capture.name), $capture.task.GetAwaiter().GetResult(), [Text.UTF8Encoding]::new($false))
                }
            } catch { $cleanupErrors += $_.Exception.Message }
        }
        $app.Dispose()
    }
    if ($app -and !$started) { $app.Dispose() }
    foreach ($key in $original.Keys) {
        if ($null -eq $original[$key]) { Remove-Item ('Env:' + $key) -ErrorAction SilentlyContinue }
        else { Set-Item ('Env:' + $key) $original[$key] }
    }
    if ($cleanupErrors.Count -eq 0) {
        foreach ($path in @($tempRoot) + @($dataRoots)) {
            if (Test-Path -LiteralPath $path) {
                try { Remove-Item -LiteralPath $path -Recurse -Force }
                catch { $cleanupErrors += $_.Exception.Message }
            }
        }
    }
    $passed = $ready -and $normalExit -and $drained -and !$forced -and !$failure -and $cleanupErrors.Count -eq 0
    $receipt = [ordered]@{
        schemaVersion=1; runId=$runId; executableSha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
        elevatedRunner=$elevated; ordinaryUserAcceptance=$false; readyProfileButtons=$visibleProfiles
        workspaceVisible=$ready; normalExit=$normalExit; nativeLifecycleMarkersVerified=$drained
        observedDescendants=$children.Count; observedDescendantsCompletionConfirmed=(!$forced -and $normalExit -and !$cleanupErrors.Count)
        sessionStarted=$false; clipboardValidated=$false; performanceValidated=$false
        remainingFixturePaths=@(@($tempRoot) + @($dataRoots) | Where-Object { Test-Path -LiteralPath $_ })
        forcedCleanup=$forced; error=$failure; cleanupErrors=$cleanupErrors; passed=$passed
    }
    [IO.File]::WriteAllText((Join-Path $out 'summary.json'), ($receipt | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    Write-Host "Window smoke receipt: $out"
}
if (!$passed) { throw "Production window smoke failed: $failure" }
