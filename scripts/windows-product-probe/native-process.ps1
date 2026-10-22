# Process ownership and UTF-8 capture, compatible with Windows PowerShell 5.1.
$script:ProductRuns = @()
$script:ProductProcess = $null
$script:ProductActiveRun = $null
$script:ProductOutput = @()
function New-ProductNativeProcess { return [Diagnostics.Process]::new() }
function Complete-ProductOutput {
    foreach ($output in $script:ProductOutput) {
        if (!$output.task.Wait(5000)) { throw 'Output did not finish after native exit.' }
        [IO.File]::WriteAllText($output.path, $output.task.GetAwaiter().GetResult(), [Text.UTF8Encoding]::new($false))
    }
    $script:ProductOutput = @()
}
function Invoke-ProductProcess([string]$Executable, [string]$Stage, [string[]]$Arguments, [string]$OutputDirectory, [int]$TimeoutMs = 120000) {
    if ($Stage -notmatch '^[a-z0-9_-]+$') { throw 'Invalid native stage.' }
    # Callers use fixed flags and checksum-verified Rust identifiers, never user commands.
    foreach ($argument in $Arguments) { if ($argument -notmatch '^[a-zA-Z0-9_:=.-]+$') { throw 'Invalid fixed native argument.' } }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executable; $info.Arguments = $Arguments -join ' '
    $info.UseShellExecute = $false; $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $info.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $script:ProductProcess = New-ProductNativeProcess
    $script:ProductProcess.StartInfo = $info
    $run = [pscustomobject]@{stage=$Stage;id=$null;started=$false;exited=$false;exitCode=$null;exitCodeAvailable=$false;timedOut=$false}
    $script:ProductRuns += $run; $script:ProductActiveRun = $run
    if (!$script:ProductProcess.Start()) { throw "Native $Stage did not start." }
    $run.started=$true; $run.id=$script:ProductProcess.Id
    $script:ProductOutput=@(
        [pscustomobject]@{path=(Join-Path $OutputDirectory ($Stage+'.log'));task=$script:ProductProcess.StandardOutput.ReadToEndAsync()},
        [pscustomobject]@{path=(Join-Path $OutputDirectory ($Stage+'-stderr.log'));task=$script:ProductProcess.StandardError.ReadToEndAsync()}
    )
    if (!$script:ProductProcess.WaitForExit($TimeoutMs)) { $run.timedOut=$true; throw "Native $Stage timed out." }
    $script:ProductProcess.WaitForExit()
    $run.exited=$script:ProductProcess.HasExited
    Complete-ProductOutput
    $code=$script:ProductProcess.ExitCode
    $run.exitCode=$code; $run.exitCodeAvailable=$code -is [int]
    if (!$run.exited -or !$run.exitCodeAvailable) { throw "Native $Stage exit code unavailable." }
    $script:ProductProcess.Dispose(); $script:ProductProcess=$null; $script:ProductActiveRun=$null
    return $code
}
function Stop-ProductProcess {
    if (!$script:ProductProcess) { return $true }
    $exited=$false
    try {
        if ($script:ProductActiveRun.started) {
            if (!$script:ProductProcess.HasExited) { $script:ProductProcess.Kill() }
            $exited=$script:ProductProcess.WaitForExit(5000)
            $script:ProductActiveRun.exited=$exited
            if ($exited) {
                $code=$script:ProductProcess.ExitCode
                $script:ProductActiveRun.exitCode=$code
                $script:ProductActiveRun.exitCodeAvailable=$code -is [int]
                Complete-ProductOutput
            }
        } else { $exited=$true }
    } finally { $script:ProductProcess.Dispose(); $script:ProductProcess=$null }
    return $exited
}
