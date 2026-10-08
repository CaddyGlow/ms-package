param(
    [Parameter(Mandatory=$true)][string]$Package,
    [Parameter(Mandatory=$true)][string]$InstalledFile,
    [Parameter(Mandatory=$true)][string]$ExpectedSha256,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory
)
$ErrorActionPreference = 'Stop'
$Package = (Resolve-Path $Package).Path
New-Item -ItemType Directory -Force $EvidenceDirectory | Out-Null
$evidence = (Resolve-Path $EvidenceDirectory).Path
$results = [ordered]@{
    os = [Environment]::OSVersion.Version.ToString()
    installer_version = (Get-Item "$env:WINDIR\System32\msi.dll").VersionInfo.FileVersion
    package_sha256 = (Get-FileHash -Algorithm SHA256 $Package).Hash.ToLowerInvariant()
}
function Run-Installer([string]$action, [string]$arguments) {
    $log = Join-Path $evidence "$action.log"
    $process = Start-Process msiexec.exe -ArgumentList "$arguments /qn /norestart /l*v `"$log`"" -Wait -PassThru
    $results[$action] = $process.ExitCode
    if ($process.ExitCode -ne 0) {throw "$action failed with $($process.ExitCode); see $log"}
}
function Verify-Payload {
    $actual = (Get-FileHash -Algorithm SHA256 $InstalledFile).Hash.ToLowerInvariant()
    if ($actual -ne $ExpectedSha256.ToLowerInvariant()) {throw "installed payload hash differs"}
    $results['payload_sha256'] = $actual
}
try {
    Run-Installer 'install' "/i `"$Package`""
    Verify-Payload
    # Removing the installed test file proves repair restores actual content.
    Remove-Item $InstalledFile
    Run-Installer 'repair' "/fa `"$Package`""
    Verify-Payload
    Run-Installer 'uninstall' "/x `"$Package`""
    if (Test-Path $InstalledFile) {throw 'uninstall left the payload behind'}
    $results['ok'] = $true
} catch {
    $results['ok'] = $false
    $results['error'] = $_.Exception.Message
    throw
} finally {
    $results | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $evidence 'result.json')
}
