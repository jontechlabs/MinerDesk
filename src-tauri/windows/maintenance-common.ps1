# Shared by the maintenance entry point and read-only regression tests.
# Windows PowerShell 5.1 compatible. No actions are executed when dot-sourced.
function Throw-MdError {
    param([int]$Code, [string]$Message)
    $failure = [System.InvalidOperationException]::new($Message)
    $failure.Data['MinerDeskExitCode'] = $Code
    throw $failure
}

function ConvertTo-MdPath {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) { return '' }
    return [System.IO.Path]::GetFullPath($Path.Trim().Trim('"')).TrimEnd('\')
}

function Test-MdSamePath {
    param([string]$Left, [string]$Right)
    if (-not $Left -or -not $Right) { return $false }
    return [string]::Equals((ConvertTo-MdPath $Left), (ConvertTo-MdPath $Right), [StringComparison]::OrdinalIgnoreCase)
}

function Read-MdRequest {
    param([string]$Path)
    $result = @{}
    foreach ($line in [IO.File]::ReadAllLines($Path, [Text.Encoding]::Unicode)) {
        if ($line -match '^\s*(?:[;#]|\[|$)') { continue }
        $split = $line.IndexOf('=')
        if ($split -lt 1) { Throw-MdError 40 'Invalid maintenance request line.' }
        $key = $line.Substring(0, $split).Trim()
        $value = $line.Substring($split + 1).Trim()
        if ($value.Length -ge 2 -and $value.StartsWith('"') -and $value.EndsWith('"')) {
            $value = $value.Substring(1, $value.Length - 2)
        }
        $result[$key] = $value
    }
    return $result
}

function Select-MdRuntime {
    param([object[]]$Processes, [string]$InstallDir, [string]$MinersRoot,
          [uint32[]]$ExcludedIds = @(), [string]$InstallerPath = '')
    $desktop = Join-Path $InstallDir 'MinerDesk.exe'
    $headless = Join-Path $InstallDir 'minerdesk-headless.exe'
    $resourceHeadless = Join-Path $InstallDir 'resources\minerdesk-headless.exe'
    $backend = Join-Path $InstallDir 'minerdesk-backend.exe'
    $resourceBackend = Join-Path $InstallDir 'resources\minerdesk-backend.exe'
    $prefix = (ConvertTo-MdPath $MinersRoot) + '\'
    foreach ($proc in $Processes) {
        if ([uint32]$proc.ProcessId -in $ExcludedIds) { continue }
        if (-not $proc.ExecutablePath) {
            # Do not turn a permission/query failure into "process still running".
            if ($proc.Name -in @('MinerDesk.exe', 'minerdesk-headless.exe', 'minerdesk-backend.exe')) {
                Throw-MdError 42 "Cannot read the executable path for PID $($proc.ProcessId) ($($proc.Name))."
            }
            continue
        }
        if (Test-MdSamePath $proc.ExecutablePath $InstallerPath) { continue }
        $path = ConvertTo-MdPath $proc.ExecutablePath
        if ((Test-MdSamePath $path $desktop) -or (Test-MdSamePath $path $headless) -or
            (Test-MdSamePath $path $resourceHeadless) -or (Test-MdSamePath $path $backend) -or
            (Test-MdSamePath $path $resourceBackend) -or $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
            $proc
        }
    }
}

function Get-MdProtectedIds {
    param([object[]]$Processes, [uint32]$HelperId, [uint32]$InstallerId, [string]$InstallDir)
    $byId = @{}
    foreach ($proc in $Processes) { $byId[[uint32]$proc.ProcessId] = $proc }
    $protected = @{}
    foreach ($start in @($HelperId, $InstallerId)) {
        $nextId = [uint32]$start
        while ($nextId -ne 0 -and -not $protected.ContainsKey($nextId)) {
            $entry = $byId[$nextId]
            # A real Desktop may be an ancestor if it opened Setup. Stop only
            # that Desktop PID, never its tree; do not exclude it as an installer.
            if ($entry -and ((Test-MdSamePath $entry.ExecutablePath (Join-Path $InstallDir 'MinerDesk.exe')) -or
                (Test-MdSamePath $entry.ExecutablePath (Join-Path $InstallDir 'minerdesk-headless.exe')) -or
                (Test-MdSamePath $entry.ExecutablePath (Join-Path $InstallDir 'minerdesk-backend.exe')))) { break }
            $protected[$nextId] = $true
            if (-not $entry) { break }
            $nextId = [uint32]$entry.ParentProcessId
        }
    }
    @($protected.Keys | ForEach-Object { [uint32]$_ })
}

function Select-MdDescendants {
    param([object[]]$Processes, [object[]]$Roots, [uint32[]]$ExcludedIds = @())
    $selected = @{}
    foreach ($root in $Roots) { $selected[[uint32]$root.ProcessId] = $root }
    do {
        $changed = $false
        foreach ($proc in $Processes) {
            $idValue = [uint32]$proc.ProcessId
            if ($idValue -in $ExcludedIds -or $selected.ContainsKey($idValue)) { continue }
            if ($selected.ContainsKey([uint32]$proc.ParentProcessId)) {
                $parent = $selected[[uint32]$proc.ParentProcessId]
                # A recycled parent PID is not proof of ownership.
                if ($parent.CreationDate -and $proc.CreationDate -and $proc.CreationDate -lt $parent.CreationDate) { continue }
                $selected[$idValue] = $proc
                $changed = $true
            }
        }
    } while ($changed)
    @($selected.Values)
}

function Test-MdSameProcess {
    param($Before, $Now)
    return ($null -ne $Now -and $Before.ProcessId -eq $Now.ProcessId -and
        $null -ne $Before.CreationDate -and $Before.CreationDate -eq $Now.CreationDate -and
        (Test-MdSamePath $Before.ExecutablePath $Now.ExecutablePath))
}

function Write-MdLog {
    param([string]$Message)
    $line = '[{0}] {1}{2}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss.fff'), $Message, [Environment]::NewLine
    [IO.File]::AppendAllText($script:MdLogPath, $line, [Text.Encoding]::Unicode)
}

function Get-MdSnapshot {
    try { @(Get-CimInstance Win32_Process -OperationTimeoutSec 10 -ErrorAction Stop) }
    catch { Throw-MdError 41 "Cannot query Windows processes: $($_.Exception.Message)" }
}

function Get-MdTask {
    try {
        $tasks = @(Get-ScheduledTask -ErrorAction Stop | Where-Object { $_.TaskPath -eq '\' -and $_.TaskName -eq 'MinerDesk Privileged Backend' })
    } catch { Throw-MdError 43 "Cannot query Task Scheduler: $($_.Exception.Message)" }
    foreach ($task in $tasks) {
        foreach ($action in @($task.Actions)) {
            $target = [Environment]::ExpandEnvironmentVariables([string]$action.Execute).Trim('"')
            if (-not (Test-MdSamePath $target (Join-Path $script:MdInstallDir 'minerdesk-headless.exe')) -and
                -not (Test-MdSamePath $target (Join-Path $script:MdInstallDir 'resources\minerdesk-headless.exe')) -and
                -not (Test-MdSamePath $target (Join-Path $script:MdInstallDir 'minerdesk-backend.exe')) -and
                -not (Test-MdSamePath $target (Join-Path $script:MdInstallDir 'resources\minerdesk-backend.exe'))) {
                Throw-MdError 21 "Backend task targets another executable ('$target'); refusing to stop an unrelated task."
            }
        }
        $task
    }
}

function Get-MdState {
    $snapshot = @(Get-MdSnapshot)
    $excluded = @(Get-MdProtectedIds -Processes $snapshot -HelperId $PID -InstallerId $script:MdInstallerId -InstallDir $script:MdInstallDir)
    $targets = @(Select-MdRuntime -Processes $snapshot -InstallDir $script:MdInstallDir -MinersRoot $script:MdMinersRoot -ExcludedIds $excluded -InstallerPath $script:MdInstallerPath)
    $task = @(Get-MdTask)
    [pscustomobject]@{ Processes = $snapshot; Targets = $targets; Excluded = $excluded; Tasks = $task }
}

function Write-MdTargets {
    param([object[]]$Targets, [string]$Label = 'Target')
    foreach ($proc in $Targets) {
        Write-MdLog ("{0}: PID={1}; PPID={2}; Name={3}; Path={4}" -f $Label, $proc.ProcessId, $proc.ParentProcessId, $proc.Name, $proc.ExecutablePath)
    }
}

function Stop-MdRuntime {
    param([switch]$RemoveTask)
    $state = Get-MdState
    $busyTask = @($state.Tasks | Where-Object { [string]$_.State -eq 'Running' }).Count -gt 0
    if (($state.Targets.Count -gt 0 -or $busyTask) -and -not $script:MdApproved) {
        Throw-MdError 10 'Runtime is active; explicit user approval is required before stopping it.'
    }
    # Capture children BEFORE stopping the task/backend, even on old versions
    # without Job Objects. Never use taskkill /T on the Desktop or its ancestors.
    $minerRoots = @($state.Targets | Where-Object {
        -not (Test-MdSamePath $_.ExecutablePath (Join-Path $script:MdInstallDir 'MinerDesk.exe'))
    })
    $tracked = @(Select-MdDescendants -Processes $state.Processes -Roots $minerRoots -ExcludedIds $state.Excluded)
    $killMap = @{}
    foreach ($proc in @($state.Targets) + $tracked) { $killMap[[uint32]$proc.ProcessId] = $proc }
    Write-MdTargets -Targets @($killMap.Values)
    foreach ($task in $state.Tasks) {
        try {
            if ($RemoveTask) { $task | Disable-ScheduledTask -ErrorAction Stop | Out-Null }
            $task | Stop-ScheduledTask -ErrorAction Stop
            if ($RemoveTask) { $task | Unregister-ScheduledTask -Confirm:$false -ErrorAction Stop }
        } catch { Throw-MdError 21 "Could not stop/remove the backend task: $($_.Exception.Message)" }
    }
    foreach ($proc in @($killMap.Values)) {
        $now = @(Get-CimInstance Win32_Process -Filter "ProcessId = $($proc.ProcessId)" -OperationTimeoutSec 10 -ErrorAction Stop)
        if ($now.Count -eq 0) { continue }
        if (-not (Test-MdSameProcess $proc $now[0])) { continue }
        try { Stop-Process -Id $proc.ProcessId -Force -ErrorAction Stop }
        catch { Write-MdLog "Stop PID $($proc.ProcessId): $($_.Exception.Message)" }
    }
    $deadline = (Get-Date).AddSeconds(15)
    $quietPasses = 0
    do {
        $after = Get-MdState
        $remaining = @($after.Targets)
        foreach ($proc in $tracked) {
            $same = @($after.Processes | Where-Object { Test-MdSameProcess $proc $_ })
            $remaining += $same
        }
        $remaining = @($remaining | Sort-Object ProcessId -Unique)
        $taskRunning = @($after.Tasks | Where-Object { [string]$_.State -eq 'Running' }).Count -gt 0
        if ($remaining.Count -eq 0 -and -not $taskRunning) {
            $quietPasses++
            if ($quietPasses -ge 2) {
                if ($RemoveTask -and $after.Tasks.Count -ne 0) { Throw-MdError 21 'The backend task registration is still present.' }
                Write-MdLog 'Verified: no installed runtime/mining process remains.'
                return
            }
        } else { $quietPasses = 0 }
        Start-Sleep -Milliseconds 350
    } while ((Get-Date) -lt $deadline)
    Write-MdTargets -Targets $remaining -Label 'REMAINING'
    if ($remaining.Count -gt 0) { Throw-MdError 20 'Confirmed runtime processes remain. Their PIDs and paths are in the log.' }
    Throw-MdError 21 'Task Scheduler still reports the backend task as running.'
}

function Invoke-MdOptionalCleanup {
    param([string]$Description, [scriptblock]$Operation)
    try { & $Operation | Out-Null; Write-MdLog "$Description completed." }
    catch { Write-MdLog "WARNING: $Description : $($_.Exception.Message)" }
}

function Remove-MdIntegrations {
    # Restrict cleanup to MinerDesk's own entries. Never remove downloaded miners
    # or the user's AppData settings. Non-critical integration failures are logged.
    Invoke-MdOptionalCleanup 'Wake tasks' {
        Get-ScheduledTask -ErrorAction Stop | Where-Object { $_.TaskName -like 'MinerDesk Wake - *' } |
            Unregister-ScheduledTask -Confirm:$false -ErrorAction Stop
    }
    Invoke-MdOptionalCleanup 'Autostart entry' {
        Remove-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'MinerDesk' -ErrorAction Stop
    }
    Invoke-MdOptionalCleanup 'MinerDesk PATH entry' {
        $oldPath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
        if ($oldPath) {
            $parts = @($oldPath -split ';' | Where-Object { $_.Trim() -and -not (Test-MdSamePath $_ $script:MdInstallDir) })
            [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'Machine')
        }
    }
    Invoke-MdOptionalCleanup 'MinerDesk firewall rules' {
        Get-NetFirewallRule -ErrorAction Stop | Where-Object {
            $_.DisplayName -like 'MinerDesk - * outbound' -or $_.DisplayName -eq 'MinerDesk - Web UI inbound'
        } | Remove-NetFirewallRule -ErrorAction Stop
    }
    Invoke-MdOptionalCleanup 'MinerDesk Defender exclusion' {
        $preferences = Get-MpPreference -ErrorAction Stop
        if (@($preferences.ExclusionPath | Where-Object { Test-MdSamePath $_ $script:MdMinersRoot }).Count -gt 0) {
            Remove-MpPreference -ExclusionPath $script:MdMinersRoot -ErrorAction Stop
        }
    }
}

function Remove-MdLegacyFiles {
    # No recursive deletion of an installation directory. A custom install path
    # can contain unrelated user files; only known MinerDesk binaries are removed.
    foreach ($relative in @('MinerDesk.exe', 'minerdesk-headless.exe', 'minerdesk-backend.exe', 'uninstall.exe', 'resources\minerdesk-headless.exe', 'resources\minerdesk-backend.exe')) {
        $file = Join-Path $script:MdInstallDir $relative
        $deadline = (Get-Date).AddSeconds(10)
        while (Test-Path -LiteralPath $file) {
            try { Remove-Item -LiteralPath $file -Force -ErrorAction Stop }
            catch {
                if ((Get-Date) -ge $deadline) { Throw-MdError 30 "Cannot remove '$file': $($_.Exception.Message)" }
                Start-Sleep -Milliseconds 300
            }
        }
    }
    Write-MdLog 'Known legacy binaries removed; other files and user data were preserved.'
}
