param([Parameter(Mandatory=$true)][string]$TestRoot,[Parameter(Mandatory=$true)][string]$FixtureExe)
$ErrorActionPreference='Stop'
foreach($name in @('run.ps1','native-process.ps1')) {
    $file=Join-Path $PSScriptRoot $name; $tokens=$null; $errors=$null
    $null=[Management.Automation.Language.Parser]::ParseFile($file,[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    if([IO.File]::ReadAllBytes($file)|Where-Object{$_ -gt 127}){throw 'PS5.1 runner must be ASCII'}
}
. (Join-Path $PSScriptRoot 'native-process.ps1')
New-Item -ItemType Directory -Path $TestRoot|Out-Null
try {
    foreach($i in 1..10){
        $fail=$i -ge 9
        $arguments=if($fail){@('--fail')}else{@()}
        $code=Invoke-ProductProcess $FixtureExe ('real-'+$i) $arguments $TestRoot
        $expected=if($fail){101}else{0}
        if($code -ne $expected -or !$script:ProductRuns[-1].exitCodeAvailable -or !$script:ProductRuns[-1].exited){throw 'real exit receipt lost'}
        $stderr=[IO.File]::ReadAllText((Join-Path $TestRoot ('real-'+$i+'-stderr.log')))
        if(!$stderr.Contains('fixture stderr') -or !$stderr.Contains([string][char]0x4e2d)){throw 'real UTF-8 stderr lost'}
    }
    function New-ProductNativeProcess {
        $p=[pscustomobject]@{StartInfo=$null;HasExited=$false;ExitCode=$null;Id=42;Killed=$false;StandardOutput=$null;StandardError=$null}
        $p|Add-Member ScriptMethod Start {
            if($global:ProductProcessMode -eq 'start-failed'){throw 'fixture denied'}
            if($this.StartInfo.StandardOutputEncoding.CodePage -ne 65001 -or $this.StartInfo.UseShellExecute){throw 'capture contract changed'}
            $this.StandardOutput=[IO.StringReader]::new('fixture stdout');$this.StandardError=[IO.StringReader]::new('fixture stderr')
            $this.ExitCode=if($global:ProductProcessMode -eq 'exit-missing'){$null}else{0}
            return $true
        }
        $p|Add-Member ScriptMethod WaitForExit {param($timeout) if($global:ProductProcessMode -eq 'timeout' -and !$this.Killed){return $false};$this.HasExited=$true;return $true}
        $p|Add-Member ScriptMethod Kill {$this.Killed=$true;$this.HasExited=$true}
        $p|Add-Member ScriptMethod Dispose {}
        return $p
    }
    foreach($mode in @('exit-missing','timeout','start-failed')){
        $global:ProductProcessMode=$mode;$threw=$false
        try{$null=Invoke-ProductProcess $FixtureExe $mode @() $TestRoot}catch{$threw=$true}
        if(!$threw){throw "false success: $mode"}
        if(!(Stop-ProductProcess)){throw 'fixture process not reclaimed'}
        if($mode -eq 'exit-missing' -and $script:ProductRuns[-1].exitCodeAvailable){throw 'missing exit promoted'}
        if($mode -eq 'timeout' -and !$script:ProductRuns[-1].timedOut){throw 'timeout receipt lost'}
    }
    foreach($bad in @('../outside','bad;command')){
        $threw=$false;try{$null=Invoke-ProductProcess $FixtureExe $bad @() $TestRoot}catch{$threw=$true}
        if(!$threw){throw 'unsafe stage accepted'}
    }
    Write-Host 'PASS process capture: 10 real fast exits, UTF-8, missing exit, timeout, start failure and path rejection'
} finally {$null=Stop-ProductProcess;Remove-Variable ProductProcessMode -Scope Global -ErrorAction SilentlyContinue}
