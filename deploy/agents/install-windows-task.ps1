# Register a per-user Task Scheduler agent. The pairing credential is supplied
# through a private file, never as a script argument or scheduled command value.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$CollectorUrl,
    [string]$AgentPath = "$env:LOCALAPPDATA\Glimdock\glimdock-agent.exe",
    [string]$StateDir = "$env:LOCALAPPDATA\Glimdock\agent",
    [string]$EnrollmentKeyFile = "$env:LOCALAPPDATA\Glimdock\enrollment.key",
    [string]$HostConfig = "$env:LOCALAPPDATA\Glimdock\host.json",
    [switch]$AllowInsecureHttp
)
$ErrorActionPreference = 'Stop'
$uri = [Uri]$CollectorUrl
if (-not $uri.IsAbsoluteUri -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or $uri.AbsolutePath -ne '/' -or $uri.Scheme -notin @('https','http')) { throw 'Use a collector base HTTPS URL without credentials, query, fragment or path.' }
$CollectorUrl = $uri.AbsoluteUri.TrimEnd('/')
if ($uri.Scheme -eq 'http' -and -not $AllowInsecureHttp) { throw 'Trusted LAN HTTP requires -AllowInsecureHttp.' }
foreach ($path in @($AgentPath,$StateDir,$EnrollmentKeyFile,$HostConfig)) {
    if (-not [IO.Path]::IsPathRooted($path) -or $path.Contains('"') -or $path.Contains("`n") -or $path.Contains("`r")) { throw 'Agent paths must be absolute and contain no quotes or control characters.' }
}
if (-not (Test-Path -LiteralPath $AgentPath -PathType Leaf)) { throw 'Install the Windows glimdock-agent.exe first.' }
if (-not (Test-Path -LiteralPath $HostConfig -PathType Leaf)) { throw 'Copy examples/agents/push-host.json to HostConfig first.' }
if (-not (Test-Path -LiteralPath (Join-Path $StateDir 'agent.json')) -and -not (Test-Path -LiteralPath $EnrollmentKeyFile -PathType Leaf)) { throw 'Create the enrollment key file before the first start.' }
$account = [Security.Principal.WindowsIdentity]::GetCurrent()
New-Item -ItemType Directory -Force -Path $StateDir | Out-Null
$acl = New-Object Security.AccessControl.DirectorySecurity
$acl.SetOwner($account.User)
$acl.SetAccessRuleProtection($true,$false)
$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($account.User,'FullControl','ContainerInherit,ObjectInherit','None','Allow')))
Set-Acl -LiteralPath $StateDir -AclObject $acl
if (Test-Path -LiteralPath $EnrollmentKeyFile -PathType Leaf) {
    $keyAcl = New-Object Security.AccessControl.FileSecurity
    $keyAcl.SetOwner($account.User)
    $keyAcl.SetAccessRuleProtection($true,$false)
    $keyAcl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($account.User,'FullControl','Allow')))
    Set-Acl -LiteralPath $EnrollmentKeyFile -AclObject $keyAcl
}
$arguments = '--collector-url "{0}" --state-dir "{1}" --enrollment-key-file "{2}" --config "{3}"' -f $CollectorUrl,$StateDir,$EnrollmentKeyFile,$HostConfig
if ($AllowInsecureHttp) { $arguments += ' --allow-insecure-http' }
$action = New-ScheduledTaskAction -Execute $AgentPath -Argument $arguments
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $account.Name
$principal = New-ScheduledTaskPrincipal -UserId $account.Name -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName 'Glimdock Agent' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Description 'Outbound native telemetry to the central Glimdock collector' -Force | Out-Null
Start-ScheduledTask -TaskName 'Glimdock Agent'
Write-Output 'Glimdock Agent scheduled for this user. Pairing credentials are read from the private enrollment file.'
