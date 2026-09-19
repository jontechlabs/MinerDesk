#requires -Version 5.1
# Read-only regression tests: no task is registered, no backend is started.
# Uses the actual source CommandAst and .NET XML Schema validation, not mocks
# that would silently accept an interval rejected by Windows Task Scheduler.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$policyPassed = 0
function Assert-MdTaskPolicyTest {
    param([bool]$Condition, [string]$Description)
    if (-not $Condition) { throw "Backend task policy regression failed: $Description" }
    $script:policyPassed++
    Write-Host "PASS: $Description"
}

$schemaPath = Join-Path $PSScriptRoot 'task-restart-interval.xsd'
function Test-MdRestartIntervalSchema {
    param([string]$Duration)
    $readerSettings = [Xml.XmlReaderSettings]::new()
    $readerSettings.ValidationType = [Xml.ValidationType]::Schema
    $readerSettings.Schemas.Add($null, $schemaPath) | Out-Null
    $readerSettings.DtdProcessing = [Xml.DtdProcessing]::Prohibit
    $escaped = [Security.SecurityElement]::Escape($Duration)
    $textReader = [IO.StringReader]::new("<Interval>$escaped</Interval>")
    $reader = $null
    try {
        $reader = [Xml.XmlReader]::Create($textReader, $readerSettings)
        while ($reader.Read()) {}
        return $true
    } catch {
        # XML validation failures reject the interval. Schema-loading failures,
        # which happen above, abort the suite rather than pass a negative test.
        return $false
    } finally {
        if ($null -ne $reader) { $reader.Dispose() }
        $textReader.Dispose()
    }
}

foreach ($invalid in @('PT0S', 'PT15S', 'PT59S', 'P32D')) {
    Assert-MdTaskPolicyTest (-not (Test-MdRestartIntervalSchema $invalid)) "Task schema rejects $invalid"
}
foreach ($valid in @('PT1M', 'PT60S', 'PT5M', 'P31D')) {
    Assert-MdTaskPolicyTest (Test-MdRestartIntervalSchema $valid) "Task schema accepts $valid"
}

$paths = @('src-tauri\windows\maintenance.ps1', 'scripts\install-privileged-backend.ps1', 'src-tauri\src\lib.rs')
$observedIntervals = @()
foreach ($relative in $paths) {
    $source = Get-Content -LiteralPath (Join-Path $projectRoot $relative) -Raw
    if ($relative.EndsWith('.rs')) {
        # Extract only the PowerShell settings statement inside Rust's repair
        # script. Never evaluate a Rust file or the registration/start commands.
        $statements = @([regex]::Matches($source, '(?m)^\$settings=New-ScheduledTaskSettingsSet[^\r\n]*'))
        Assert-MdTaskPolicyTest ($statements.Count -eq 1) 'Rust repair script exposes exactly one task-settings statement'
        $source = $statements[0].Value
    }
    $tokens = $null
    $parseErrors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$parseErrors)
    Assert-MdTaskPolicyTest (@($parseErrors).Count -eq 0) "Settings source parses: $relative"
    $commands = @($ast.FindAll({
        param($node)
        $node -is [Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq 'New-ScheduledTaskSettingsSet'
    }, $true))
    Assert-MdTaskPolicyTest ($commands.Count -eq 1) "Exactly one settings call: $relative"
    $commandText = $commands[0].Extent.Text
    $intervalMatch = [regex]::Match($commandText, '(?i)-RestartInterval\s+\(New-TimeSpan\s+-(Seconds|Minutes|Hours|Days)\s+([0-9]+)\)')
    $countMatch = [regex]::Match($commandText, '(?i)-RestartCount\s+([0-9]+)\b')
    Assert-MdTaskPolicyTest ($intervalMatch.Success -and $countMatch.Success) "Explicit restart policy is extractable: $relative"
    $value = [double]$intervalMatch.Groups[2].Value
    $interval = switch ($intervalMatch.Groups[1].Value.ToLowerInvariant()) {
        'seconds' { [TimeSpan]::FromSeconds($value) }
        'minutes' { [TimeSpan]::FromMinutes($value) }
        'hours'   { [TimeSpan]::FromHours($value) }
        'days'    { [TimeSpan]::FromDays($value) }
    }
    $xmlDuration = [Xml.XmlConvert]::ToString([TimeSpan]$interval)
    Assert-MdTaskPolicyTest (Test-MdRestartIntervalSchema $xmlDuration) "Production restart interval meets Windows schema: $relative ($xmlDuration)"
    Assert-MdTaskPolicyTest ($interval.TotalSeconds -eq 60 -and [int]$countMatch.Groups[1].Value -eq 3) "Policy is three retries, one minute apart: $relative"
    Assert-MdTaskPolicyTest ($commandText -match '-ExecutionTimeLimit\s+\(\[TimeSpan\]::Zero\)' -and $commandText -match '-MultipleInstances\s+IgnoreNew') "Unlimited runtime and single-instance settings preserved: $relative"
    $observedIntervals += $interval.TotalSeconds
}
Assert-MdTaskPolicyTest (@($observedIntervals | Select-Object -Unique).Count -eq 1) 'Installer, CLI installer and Desktop repair use the same restart interval'
Write-Host "Backend task policy preflight passed: $policyPassed assertions. No task was registered or started." -ForegroundColor Green
