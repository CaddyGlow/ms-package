$ErrorActionPreference='Stop'
$root='C:\Users\deploy\msi-refactor-windows-tests'
New-Item -ItemType Directory -Force "$root\evidence" | Out-Null
$results=@()
foreach($test in (Get-Content -Raw "$root\executables.json" | ConvertFrom-Json)) {
 $name="$($test.domain)-$($test.target)-$($test.file)"
 $file="$root\$($test.file)"
 $actual=(Get-FileHash -Algorithm SHA256 $file).Hash.ToLowerInvariant()
 if($actual -ne $test.sha256){throw "test executable hash differs"}
 $p=Start-Process -FilePath $file -ArgumentList '--test-threads=1' -WorkingDirectory $root -RedirectStandardOutput "$root\evidence\$name.stdout.log" -RedirectStandardError "$root\evidence\$name.stderr.log" -PassThru
 $handle=$p.Handle
 $completed=$p.WaitForExit(120000)
 if(-not $completed){$p.Kill();$p.WaitForExit();$code=-1}else{$p.WaitForExit();$code=$p.ExitCode}
 $results+=@{domain=$test.domain;target=$test.target;file=$test.file;sha256=$actual;exit_code=$code;timed_out=(-not $completed)}
 $results|ConvertTo-Json|Set-Content "$root\test-results.json"
 Write-Output "$($test.domain) $($test.target): $code"
}
