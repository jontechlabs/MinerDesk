#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$RequestPath,
    [string]$Action = 'Inspect',
    [string]$InstallDir,
    [string]$LogPath,
    [uint32]$InstallerProcessId = 0,
    [string]$InstallerPath = '',
    [string]$InstalledVersion = '',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$script:MdLogPath = $null
$exitCode = 50
try {
    . (Join-Path $PSScriptRoot 'maintenance-common.ps1')
    $approved = [bool]$Force
    if ($RequestPath) {
        $request = Read-MdRequest $RequestPath
        foreach ($required in @('Action', 'InstallDir', 'LogPath', 'InstallerProcessId', 'InstallerPath', 'Approved')) {
            if (-not $request.ContainsKey($required)) { Throw-MdError 40 "Request field missing: $required" }
        }
        $Action = $request['Action']
        $InstallDir = $request['InstallDir']
        $LogPath = $request['LogPath']
        $InstallerProcessId = [uint32]$request['InstallerProcessId']
        $InstallerPath = $request['InstallerPath']
        $InstalledVersion = [string]$request['InstalledVersion']
        $approved = ($request['Approved'] -eq '1')
    }
    if (-not $LogPath) { $LogPath = Join-Path $env:ProgramData 'MinerDesk\logs\setup-maintenance.log' }
    $script:MdLogPath = [IO.Path]::GetFullPath($LogPath)
    [IO.Directory]::CreateDirectory((Split-Path -Parent $script:MdLogPath)) | Out-Null
    Write-MdLog "Helper 0.7.22 started: action=$Action; PID=$PID; PowerShell=$($PSVersionTable.PSVersion); Is64Bit=$([Environment]::Is64BitProcess)"
    if (-not $InstallDir) { $InstallDir = Join-Path $env:ProgramFiles 'MinerDesk' }
    if ($InstallDir -notmatch '^(?:[A-Za-z]:[\\/]|\\\\)') { Throw-MdError 40 'InstallDir must be an absolute Windows path.' }
    $script:MdInstallDir = ConvertTo-MdPath $InstallDir
    if ($script:MdInstallDir -eq [IO.Path]::GetPathRoot($script:MdInstallDir).TrimEnd('\')) {
        Throw-MdError 40 'A drive root is not a valid MinerDesk install directory.'
    }
    $script:MdMinersRoot = ConvertTo-MdPath (Join-Path $env:ProgramData 'MinerDesk\miners')
    $script:MdInstallerId = $InstallerProcessId
    $script:MdInstallerPath = $InstallerPath
    $script:MdApproved = $approved
    Write-MdLog "InstallDir=$script:MdInstallDir; MinersRoot=$script:MdMinersRoot; InstallerPID=$InstallerProcessId; InstallerPath=$InstallerPath; Approved=$approved"
    switch ($Action) {
        'Inspect' {
            $state = Get-MdState
            Write-MdTargets -Targets $state.Targets
            $runningTask = @($state.Tasks | Where-Object { [string]$_.State -eq 'Running' }).Count -gt 0
            Write-MdLog "Inspection: $($state.Targets.Count) runtime process(es); taskRunning=$runningTask."
            if ($state.Targets.Count -gt 0 -or $runningTask) { $exitCode = 10 } else { $exitCode = 0 }
        }
        'Stop' { Stop-MdRuntime; $exitCode = 0 }
        'PrepareInstall' { Stop-MdRuntime -RemoveTask; $exitCode = 0 }
        'PrepareUninstall' {
            Stop-MdRuntime -RemoveTask
            Remove-MdIntegrations
            $exitCode = 0
        }
        'LegacyCleanup' {
            if (-not $approved) { Throw-MdError 10 'Legacy cleanup requires explicit approval.' }
            Write-MdLog "Legacy version=$InstalledVersion"
            Stop-MdRuntime -RemoveTask
            Remove-MdIntegrations
            Remove-MdLegacyFiles
            $exitCode = 0
        }
        'InstallBackend' {
            if (-not $approved) { Throw-MdError 10 'Backend installation requires approval.' }
            $headless = Join-Path $script:MdInstallDir 'minerdesk-backend.exe'
            if (-not (Test-Path -LiteralPath $headless -PathType Leaf)) { Throw-MdError 60 "Windowless backend binary is missing: $headless" }
            Stop-MdRuntime -RemoveTask
            $userName = (Get-CimInstance Win32_ComputerSystem -ErrorAction Stop).UserName
            if ([string]::IsNullOrWhiteSpace($userName)) { $userName = [Security.Principal.WindowsIdentity]::GetCurrent().Name }
            $taskAction = New-ScheduledTaskAction -Execute $headless -Argument '--desktop-owned' -WorkingDirectory $script:MdInstallDir
            $trigger = New-ScheduledTaskTrigger -AtLogOn -User $userName
            $principal = New-ScheduledTaskPrincipal -UserId $userName -LogonType Interactive -RunLevel Highest
            # RestartOnFailure/Interval is restricted by the Windows task schema
            # to PT1M..P31D. PT15S is rejected during Register-ScheduledTask.
            $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
            Write-MdLog "Registering backend task: user=$userName; executable=$headless; arguments=--desktop-owned; RestartCount=3; RestartInterval=PT1M; ExecutionTimeLimit=PT0S; MultipleInstances=IgnoreNew."
            try {
                Register-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -Action $taskAction -Trigger $trigger -Principal $principal -Settings $settings -Description 'MinerDesk privileged mining backend' -Force -ErrorAction Stop | Out-Null
            } catch {
                Throw-MdError 61 "Register-ScheduledTask failed (RestartInterval=PT1M): $($_.Exception.Message)"
            }
            $registered = @(Get-MdTask)
            if ($registered.Count -ne 1) { Throw-MdError 61 'The new backend task registration could not be verified.' }
            Write-MdLog "Backend task registered: RestartCount=$($registered[0].Settings.RestartCount); RestartInterval=$($registered[0].Settings.RestartInterval)."
            # An XML export aids diagnosis but is not a prerequisite for starting.
            try {
                $xmlPath = Join-Path (Split-Path -Parent $script:MdLogPath) 'backend-task.xml'
                $taskXml = Export-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction Stop
                [IO.File]::WriteAllText($xmlPath, [string]$taskXml, [Text.Encoding]::Unicode)
                Write-MdLog "Registered task XML: $xmlPath"
            } catch { Write-MdLog "Task XML export unavailable: $($_.Exception.Message)" }
            try {
                Start-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction Stop
            } catch {
                Throw-MdError 62 "The backend task was registered, but Start-ScheduledTask failed: $($_.Exception.Message)"
            }
            Write-MdLog "Backend task start requested for $userName, with --desktop-owned."
            $exitCode = 0
        }
        'AddPath' {
            if (-not $approved) { Throw-MdError 10 'Changing PATH requires approval.' }
            $oldPath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
            $parts = @($oldPath -split ';' | Where-Object { $_.Trim() })
            if (@($parts | Where-Object { Test-MdSamePath $_ $script:MdInstallDir }).Count -eq 0) {
                $parts += $script:MdInstallDir
                [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'Machine')
            }
            $exitCode = 0
        }
        'AddDefender' {
            if (-not $approved) { Throw-MdError 10 'Adding a Defender exclusion requires approval.' }
            [IO.Directory]::CreateDirectory($script:MdMinersRoot) | Out-Null
            Add-MpPreference -ExclusionPath $script:MdMinersRoot -ErrorAction Stop
            $preferences = Get-MpPreference -ErrorAction Stop
            if (@($preferences.ExclusionPath | Where-Object { Test-MdSamePath $_ $script:MdMinersRoot }).Count -eq 0) {
                Throw-MdError 60 'The Defender exclusion could not be verified.'
            }
            $exitCode = 0
        }
        default { Throw-MdError 40 "Unknown maintenance action: $Action" }
    }
    Write-MdLog "Action $Action finished; exit=$exitCode."
    Write-Output "MinerDesk maintenance $Action : exit $exitCode. Log: $script:MdLogPath"
} catch {
    $exception = $_.Exception
    $exitCode = 50
    while ($null -ne $exception) {
        if ($exception.Data.Contains('MinerDeskExitCode')) { $exitCode = [int]$exception.Data['MinerDeskExitCode']; break }
        $exception = $exception.InnerException
    }
    $details = "ERROR action=$Action exit=$exitCode : $($_.Exception.Message)`r`n$($_.InvocationInfo.PositionMessage)"
    if ($script:MdLogPath) {
        try { [IO.File]::AppendAllText($script:MdLogPath, $details + [Environment]::NewLine, [Text.Encoding]::Unicode) } catch {}
    }
    # NSIS captures this even if the script cannot open its log.
    [Console]::Error.WriteLine($details)
}
exit $exitCode
