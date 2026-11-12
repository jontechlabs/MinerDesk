$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$request = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:MINERDESK_UPDATE_NATIVE_REQUEST)) | ConvertFrom-Json
Remove-Item Env:MINERDESK_UPDATE_NATIVE_REQUEST
$admins = 'S-1-5-32-544'
$system = 'S-1-5-18'
$installer = 'S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464'
function Assert-ProtectedItem([string]$path, [bool]$ancestor) {
    $item = Get-Item -LiteralPath $path -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Reparse points are not allowed in update paths' }
    $acl = Get-Acl -LiteralPath $path
    $owner = $acl.GetOwner([Security.Principal.SecurityIdentifier]).Value
    if ($owner -notin @($admins,$system,$installer)) { throw 'Update path is not owned by a protected system identity' }
    $writes = [Security.AccessControl.FileSystemRights]::Write -bor [Security.AccessControl.FileSystemRights]::Delete -bor [Security.AccessControl.FileSystemRights]::DeleteSubdirectoriesAndFiles -bor [Security.AccessControl.FileSystemRights]::ChangePermissions -bor [Security.AccessControl.FileSystemRights]::TakeOwnership
    if ($ancestor) {
        # Permission to create a sibling cannot replace a protected existing
        # child. Delete-child, delete, owner and DACL writes can redirect it.
        $writes = [Security.AccessControl.FileSystemRights]::Delete -bor [Security.AccessControl.FileSystemRights]::DeleteSubdirectoriesAndFiles -bor [Security.AccessControl.FileSystemRights]::ChangePermissions -bor [Security.AccessControl.FileSystemRights]::TakeOwnership
    }
    foreach ($rule in $acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])) {
        if ($rule.AccessControlType -eq 'Allow' -and ($rule.PropagationFlags -band [Security.AccessControl.PropagationFlags]::InheritOnly) -eq 0 -and $rule.IdentityReference.Value -notin @($admins,$system,$installer) -and ($rule.FileSystemRights -band $writes) -ne 0) { throw 'Update path can be modified by a non-administrator' }
    }
}
function Assert-Protected([string]$path) {
    $full = [IO.Path]::GetFullPath($path)
    Assert-ProtectedItem $full $false
    $next = [IO.Directory]::GetParent($full)
    while ($null -ne $next) {
        Assert-ProtectedItem $next.FullName $true
        $next = $next.Parent
    }
}
function New-ProtectedFile([string]$path, [bool]$public) {
    Assert-Protected (Split-Path -Parent $path)
    $acl = New-Object Security.AccessControl.FileSecurity
    $acl.SetAccessRuleProtection($true,$false)
    $acl.SetOwner((New-Object Security.Principal.SecurityIdentifier($admins)))
    foreach ($sid in @($admins,$system)) { $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule((New-Object Security.Principal.SecurityIdentifier($sid)),'FullControl','Allow'))) }
    if ($public) { $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule((New-Object Security.Principal.SecurityIdentifier('S-1-5-32-545')),'ReadAndExecute','Allow'))) }
    # FileStream's security constructor supplies the DACL and owner to the
    # create operation. A split-token user never owns an intermediate file.
    return New-Object IO.FileStream($path,[IO.FileMode]::CreateNew,[Security.AccessControl.FileSystemRights]::Write,[IO.FileShare]::None,4096,[IO.FileOptions]::None,$acl)
}
function Protect-NewDirectory([string]$path, [bool]$public) {
    if (Test-Path -LiteralPath $path) { Assert-Protected $path; return }
    Assert-Protected (Split-Path -Parent $path)
    $acl = New-Object Security.AccessControl.DirectorySecurity
    $acl.SetAccessRuleProtection($true,$false)
    $acl.SetOwner((New-Object Security.Principal.SecurityIdentifier($admins)))
    foreach ($sid in @($admins,$system)) { $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule((New-Object Security.Principal.SecurityIdentifier($sid)),'FullControl','ContainerInherit,ObjectInherit','None','Allow'))) }
    if ($public) { $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule((New-Object Security.Principal.SecurityIdentifier('S-1-5-32-545')),'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow'))) }
    # Creation with the ACL is atomic: an ordinary user never owns this name.
    [IO.Directory]::CreateDirectory($path,$acl) | Out-Null
    Assert-Protected $path
}
switch ($request.action) {
    'inspect' {
        Assert-Protected $request.install
        foreach ($name in @('minerdesk-backend.exe','resources\minerdesk-update-helper.exe','resources\maintenance.ps1','resources\maintenance-common.ps1')) { Assert-Protected (Join-Path $request.install $name) }
        $process = Get-CimInstance Win32_Process -Filter ('ProcessId = ' + [uint32]$request.pid) -OperationTimeoutSec 10
        if (!$process -or !$process.ExecutablePath -or [IO.Path]::GetFullPath($process.ExecutablePath) -ine [IO.Path]::GetFullPath((Join-Path $request.install 'minerdesk-backend.exe'))) { throw 'Only the installed backend can prepare a remote update' }
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        $owner = Invoke-CimMethod -InputObject $process -MethodName GetOwnerSid
        if ($owner.ReturnValue -ne 0 -or $owner.Sid -ne $identity.User.Value) { throw 'Original backend identity differs from the worker' }
        $task = Get-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -TaskPath '\' -ErrorAction Stop
        $taskUser = [string]$task.Principal.UserId
        $taskSid = if ($taskUser.StartsWith('S-1-')) { (New-Object Security.Principal.SecurityIdentifier($taskUser)).Value } else { (New-Object Security.Principal.NTAccount($taskUser)).Translate([Security.Principal.SecurityIdentifier]).Value }
        if ($taskSid -ne $identity.User.Value -or $task.Principal.LogonType -ne 'Interactive' -or $task.Principal.RunLevel -ne 'Highest' -or @($task.Actions).Count -ne 1 -or [IO.Path]::GetFullPath($task.Actions[0].Execute) -ine [IO.Path]::GetFullPath($process.ExecutablePath)) { throw 'The existing backend task does not match this original user and installation' }
        $base = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'MinerDesk'
        Assert-Protected $base
        $base = Join-Path $base 'updates'
        Protect-NewDirectory $base $false
        Protect-NewDirectory (Join-Path $base 'public') $true
        [pscustomobject]@{pid=[uint32]$process.ProcessId; creation=$process.CreationDate.ToUniversalTime().Ticks.ToString(); install=$request.install; base=$base; version=(Get-Item -LiteralPath $process.ExecutablePath).VersionInfo.ProductVersion; task_xml=(Export-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -TaskPath '\'); command_line=[string]$process.CommandLine; task_arguments=[string]$task.Actions[0].Arguments} | ConvertTo-Json -Compress
    }
    'stage' {
        Assert-Protected $request.base
        Protect-NewDirectory $request.directory $false
        Assert-Protected $request.source
        $source = [IO.File]::OpenRead($request.source)
        $target = New-ProtectedFile (Join-Path $request.directory 'minerdesk-update-helper.exe') $false
        try { $source.CopyTo($target); $target.Flush($true) } finally { $target.Dispose(); $source.Dispose() }
        Assert-Protected (Join-Path $request.directory 'minerdesk-update-helper.exe')
        $target = New-ProtectedFile (Join-Path $request.directory 'context.json') $false
        try { $data = [Text.Encoding]::UTF8.GetBytes($request.context); $target.Write($data,0,$data.Length); $target.Flush($true) } finally { $target.Dispose() }
        Assert-Protected (Join-Path $request.directory 'context.json')
    }
    'allocate' {
        $target = New-ProtectedFile $request.path ([bool]$request.public)
        $target.Dispose()
        Assert-Protected $request.path
    }
    'seal' {
        Assert-Protected $request.path
    }
    'publish' {
        Assert-Protected $request.source
        Assert-Protected (Split-Path -Parent $request.path)
        if (Test-Path -LiteralPath $request.path) {
            Assert-Protected $request.path
            [IO.File]::Replace($request.source,$request.path,$null)
        } else {
            [IO.File]::Move($request.source,$request.path)
        }
    }
    'validate' {
        Assert-Protected $request.directory
        Assert-Protected (Join-Path $request.directory 'context.json')
        Assert-Protected (Join-Path $request.directory 'minerdesk-update-helper.exe')
        Assert-Protected $request.install
        $process = Get-CimInstance Win32_Process -Filter ('ProcessId = ' + [uint32]$request.pid) -OperationTimeoutSec 10
        if (!$process -or $process.CreationDate.ToUniversalTime().Ticks.ToString() -ne $request.creation -or [IO.Path]::GetFullPath($process.ExecutablePath) -ine [IO.Path]::GetFullPath((Join-Path $request.install 'minerdesk-backend.exe'))) { throw 'Original backend identity changed during worker detachment' }
    }
    'prepare' {
        Assert-Protected $request.install
        $script = Join-Path $request.install 'resources\maintenance.ps1'
        Assert-Protected $script
        Assert-Protected (Join-Path $request.install 'resources\maintenance-common.ps1')
        # The ready/ack handover has already stopped miners and exited the
        # original backend. This installed action closes only its known runtime.
        & $script -Action PrepareInstall -InstallDir $request.install -LogPath (Join-Path $request.base 'maintenance.log') -Force
        if ($LASTEXITCODE -ne 0) { throw ('Installed maintenance failed: ' + $LASTEXITCODE) }
    }
    'restart' {
        Assert-Protected $request.install
        Assert-Protected (Join-Path $request.install 'minerdesk-backend.exe')
        # Restore the original OS task XML captured from the validated user,
        # executable, interactive logon and run level before installing.
        Register-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -TaskPath '\' -Xml $request.task_xml -Force | Out-Null
        Start-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -TaskPath '\'
    }
    default { throw 'Unsupported native worker action' }
}
