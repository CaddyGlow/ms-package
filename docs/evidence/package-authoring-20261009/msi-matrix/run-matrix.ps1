$ErrorActionPreference='Stop'
$root='C:\Users\deploy\msi-matrix'
$results=@()
foreach($cell in (Get-Content -Raw "$root\cells.json" | ConvertFrom-Json)) {
 $base=if($cell.context -eq 'user'){$env:LOCALAPPDATA}elseif($cell.arch -eq 'x86'){${env:ProgramFiles(x86)}}else{$env:ProgramFiles}
 $installed=Join-Path $base "ms-package-matrix-$($cell.name)"
 $args=@{Package="$root\$($cell.name)\package.msi";InstalledFile="$installed\payload.txt";ExpectedSha256=$cell.payload_sha256;EvidenceDirectory="$root\evidence\$($cell.name)"}
 if($cell.media -eq 'split'){,@{path="$installed\second.txt";sha256=$cell.second_sha256}|ConvertTo-Json|Set-Content "$root\$($cell.name)\additional.json";$args.AdditionalPayloadManifest="$root\$($cell.name)\additional.json"}
 try { & 'C:\Users\deploy\qualify-installer-matrix.ps1' @args; $results+=@{name=$cell.name;ok=$true};Write-Output "$($cell.name): PASS" } catch {$results+=@{name=$cell.name;ok=$false;error=$_.Exception.Message};Write-Output "$($cell.name): FAIL $($_.Exception.Message)"}
 $results|ConvertTo-Json|Set-Content "$root\matrix-result.json"
}
