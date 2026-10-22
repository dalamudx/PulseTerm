# Read-only, bounded evidence for renderer STATUS_ACCESS_DENIED. Windows PowerShell 5.1.
function Save-StartupDiagnostics {
    param([string]$Output, [string]$Kit, [string]$Profile, [string]$Runtime, [datetime]$Started)
    $paths = @($Kit, (Join-Path $Kit 'echo-webview2.exe'), (Join-Path $Kit 'WebView2Loader.dll'),
        (Join-Path $Kit 'echo.html'), (Join-Path $Kit 'xterm'), $Profile)
    $paths += @(Get-ChildItem -LiteralPath $Profile -Directory | ForEach-Object { $_.FullName })
    if ($Runtime) { $paths += $Runtime; $paths += Join-Path $Runtime 'msedgewebview2.exe' }
    # The host records the actual browser image, including an explicitly selected runtime.
    foreach ($log in Get-ChildItem -LiteralPath $Output -Filter '*-host.log') {
        foreach ($line in Get-Content -LiteralPath $log.FullName) {
            if ($line.StartsWith('runtime_image=')) {
                $image = $line.Substring(14)
                if ($image -and [IO.Path]::IsPathRooted($image)) { $paths += $image }
            }
        }
    }
    $acls = @(); $seen = @{}
    foreach ($path in $paths) {
        # Include ancestors: access to the final file alone does not establish sandbox traversal.
        for ($depth = 0; $path -and $depth -lt 16; $depth++) {
            if ($seen.ContainsKey($path)) { break }
            $seen[$path] = $true
            try {
                $acl = Get-Acl -LiteralPath $path -ErrorAction Stop
                $acls += [pscustomobject]@{ path=$path; status='read'; sddl=$acl.Sddl; error=$null }
            } catch {
                $acls += [pscustomobject]@{ path=$path; status='unavailable'; sddl=$null; error=$_.FullyQualifiedErrorId }
            }
            $path = Split-Path -Path $path -Parent
        }
    }
    $ended = Get-Date
    $logs = @('Application', 'Microsoft-Windows-CodeIntegrity/Operational',
        'Microsoft-Windows-AppLocker/EXE and DLL', 'Microsoft-Windows-Windows Defender/Operational')
    $queries = @(); $events = @()
    foreach ($name in $logs) {
        try {
            $filter = @{ LogName=$name; StartTime=$Started.AddSeconds(-2); EndTime=$ended }
            if ($name -eq 'Application') { $filter.Id = @(1000, 1001) }
            $recent = @(Get-WinEvent -FilterHashtable $filter -MaxEvents 128 -ErrorAction Stop)
            $matched = 0
            foreach ($event in $recent) {
                $xml = [xml]$event.ToXml()
                if ($xml.OuterXml -notmatch '(?i)msedgewebview2|echo-webview2|WebView2Loader') { continue }
                if ($matched -ge 20) { break }
                $data = @()
                foreach ($field in $xml.Event.EventData.Data) {
                    # Exclude command lines, general messages, dump paths and unrelated event fields.
                    if ($field.Name -notmatch '^(AppName|AppVersion|ModuleName|ModuleVersion|ExceptionCode|Status|FileName|FilePath|ProcessName|ProcessId|FailureStatus|PolicyName|RuleName)$') { continue }
                    $value = ([string]$field.InnerText) -replace '[\x00-\x1f]', '?'
                    if ($value.Length -gt 512) { $value = $value.Substring(0, 512) }
                    $data += [pscustomobject]@{ name=[string]$field.Name; value=$value }
                }
                $events += [pscustomobject]@{ log=$name; id=$event.Id; provider=$event.ProviderName; time=$event.TimeCreated; data=$data }
                $matched++
            }
            $queries += [pscustomobject]@{ log=$name; status='read'; inspected=$recent.Count; matched=$matched; limitReached=($recent.Count -eq 128); error=$null }
        } catch {
            $empty = $_.FullyQualifiedErrorId -like '*NoMatchingEventsFound*'
            $queries += [pscustomobject]@{ log=$name; status=$(if ($empty) { 'no-events' } else { 'unavailable' }); inspected=0; matched=0; limitReached=$false; error=$_.FullyQualifiedErrorId }
        }
    }
    [pscustomobject]@{ schemaVersion=1; started=$Started; ended=$ended; acls=$acls; eventQueries=$queries; events=$events } |
        ConvertTo-Json -Depth 7 | Set-Content -Encoding UTF8 (Join-Path $Output 'startup-system.json')
}
