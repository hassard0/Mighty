# demos/04_kvstore/smoke.ps1 — PowerShell equivalent of smoke.sh.
# Exits 0 on pass, non-zero on fail.

$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot/../..").Path
$mty = $env:MTY
if (-not $mty) { $mty = Join-Path $root "target\debug\mty.exe" }
if (-not (Test-Path $mty)) {
    Write-Error "smoke: mty binary not found at $mty. Build with: cargo build -p mty-cli"
    exit 2
}

$demo = Join-Path $root "demos\04_kvstore\src\main.mty"
# Use the .NET Process API directly so we sidestep PowerShell's
# `2>&1` -> NativeCommandError wrapping (which mangles native-exe
# panic-on-stdout output into ErrorRecord objects) and the random
# CLR-startup line that some PowerShell hosts inject.
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $mty
$psi.Arguments = "run `"$demo`""
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true
$p = [System.Diagnostics.Process]::Start($psi)
$stdout = $p.StandardOutput.ReadToEnd()
$stderr = $p.StandardError.ReadToEnd()
$p.WaitForExit()
$out = $stdout + $stderr

$expectations = @(
    @{ Label = "boot";                Needle = 'spawned: counter, 3 shards, coordinator, frontend' },
    @{ Label = "put_alpha_shard1";    Needle = '{"shard":1,"k":"alpha","v":"1","ok":1}' },
    @{ Label = "put_bravo_shard0";    Needle = '{"shard":0,"k":"bravo","v":"2","ok":1}' },
    @{ Label = "put_charlie_shard2";  Needle = '{"shard":2,"k":"charlie","v":"3","ok":1}' },
    @{ Label = "put_delta_shard1";    Needle = '{"shard":1,"k":"delta","v":"4","ok":1}' },
    @{ Label = "put_echo_shard0";     Needle = '{"shard":0,"k":"echo","v":"5","ok":1}' },
    @{ Label = "put_foxtrot_shard2";  Needle = '{"shard":2,"k":"foxtrot","v":"6","ok":1}' },
    @{ Label = "get_alpha_hit";       Needle = '{"shard":1,"k":"alpha","hit":true,"v":"1"}' },
    @{ Label = "get_foxtrot_hit";     Needle = '{"shard":2,"k":"foxtrot","hit":true,"v":"6"}' },
    @{ Label = "miss_ghost";          Needle = '{"shard":2,"k":"ghost","hit":false}' },
    @{ Label = "del_bravo";           Needle = '{"shard":0,"k":"bravo","removed":1}' },
    @{ Label = "del_then_miss";       Needle = '{"shard":0,"k":"bravo","hit":false}' },
    @{ Label = "crash_panic";         Needle = 'panic: shard 1 crashed on purpose' },
    @{ Label = "crash_trapped";       Needle = '{"crashed_shard":1,"status":"trapped"}' },
    @{ Label = "post_crash_alpha";    Needle = '{"shard":1,"k":"alpha","hit":true,"v":"1"}' },
    @{ Label = "post_crash_charlie";  Needle = '{"shard":2,"k":"charlie","hit":true,"v":"3"}' },
    @{ Label = "post_crash_delta";    Needle = '{"shard":1,"k":"delta","hit":true,"v":"4"}' },
    @{ Label = "stats";               Needle = '"shards":[1,2,2]' },
    @{ Label = "metrics_shape";       Needle = '"metrics":{"puts":' },
    @{ Label = "http_put";            Needle = '"PUT":{"shard":1,"k":"http_key","v":"http_val","ok":1}' },
    @{ Label = "http_get";            Needle = '"GET":{"shard":1,"k":"http_key","hit":true,"v":"http_val"}' },
    @{ Label = "http_del";            Needle = '"DELETE":{"shard":1,"k":"http_key","removed":1}' }
)

$fail = 0
foreach ($e in $expectations) {
    # Use plain substring containment (-like would treat `[` as a
    # wildcard char and miss the stats line's "[1,2,2]" shape).
    if (-not $out.Contains($e.Needle)) {
        Write-Host "smoke FAIL [$($e.Label)]: expected output to contain: $($e.Needle)" -ForegroundColor Red
        $fail = 1
    }
}

if ($fail -ne 0) {
    Write-Host "---- captured output ----"
    Write-Host $out
    exit 1
}
Write-Host "04_kvstore: PASS"
