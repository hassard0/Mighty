# scripts/release.ps1 — single-command release workflow for Mighty
# (PowerShell port of scripts/release.sh).
#
# Usage:
#   scripts/release.ps1 [-DryRun] <new-version>
#
# Example:
#   scripts/release.ps1 0.9.0
#   scripts/release.ps1 -DryRun 0.9.0-rc1
#
# Steps:
#   1. Verify clean working tree.
#   2. `cargo test --workspace`.
#   3. Bump workspace.package.version.
#   4. Prepend CHANGELOG stub.
#   5. Commit + tag + push.
#   6. Bundle + sign publishable packages (no-op marketplace upload in v0.9).

[CmdletBinding()]
param(
    [switch] $DryRun,
    [Parameter(Mandatory = $true, Position = 0)] [string] $NewVersion
)

$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $Root

$CargoToml = Join-Path $Root "Cargo.toml"
$Changelog = Join-Path $Root "CHANGELOG.md"
$Tag = "v$NewVersion"

function Say($msg) { Write-Host "[release] $msg" -ForegroundColor Cyan }
function Die($msg) { Write-Host "[release] $msg" -ForegroundColor Red; exit 1 }

# ----------------------------------------------------------------
# Step 1 — clean tree.
# ----------------------------------------------------------------
Say "step 1/6: verify clean working tree"
$status = git status --porcelain
if ($status) {
    Die "working tree is dirty; commit or stash before releasing"
}
git rev-parse $Tag 2>$null
if ($LASTEXITCODE -eq 0) {
    Die "tag $Tag already exists"
}

# ----------------------------------------------------------------
# Step 2 — test.
# ----------------------------------------------------------------
Say "step 2/6: cargo test --workspace"
cargo test --workspace
if ($LASTEXITCODE -ne 0) { Die "tests failed" }

# ----------------------------------------------------------------
# Step 3 — bump version.
# ----------------------------------------------------------------
$text = Get-Content -Raw -Encoding utf8 $CargoToml
$match = [regex]::Match($text, '(\[workspace\.package\][^\[]*?version\s*=\s*")[^"]+(")', `
    [System.Text.RegularExpressions.RegexOptions]::Singleline)
if (-not $match.Success) {
    Die "could not locate workspace.package version line"
}
$current = [regex]::Match($text.Substring($match.Index), 'version\s*=\s*"([^"]+)"').Groups[1].Value

Say "step 3/6: bump version $current -> $NewVersion"
if ($DryRun) {
    Say "  (dry-run) would edit $CargoToml"
} else {
    $newText = $text.Substring(0, $match.Index) + $match.Groups[1].Value + $NewVersion `
        + $match.Groups[2].Value + $text.Substring($match.Index + $match.Length)
    Set-Content -Encoding utf8 -NoNewline -Path $CargoToml -Value $newText
}

# ----------------------------------------------------------------
# Step 4 — changelog.
# ----------------------------------------------------------------
Say "step 4/6: prepend CHANGELOG entry"
if ($DryRun) {
    Say "  (dry-run) would prepend a stub for $Tag to $Changelog"
} else {
    $date = (Get-Date -Format "yyyy-MM-dd")
    $stub = "## [$NewVersion] - $date`n`n- TODO: fill in release notes.`n"
    if (Test-Path $Changelog) {
        $existing = Get-Content -Raw -Encoding utf8 $Changelog
        $head, $tail = $existing -split "`n", 2
        $merged = "$head`n`n$stub`n$tail"
        Set-Content -Encoding utf8 -NoNewline -Path $Changelog -Value $merged
    } else {
        $body = "# Changelog`n`n$stub`n"
        Set-Content -Encoding utf8 -NoNewline -Path $Changelog -Value $body
    }
}

# ----------------------------------------------------------------
# Step 5 — commit + tag + push.
# ----------------------------------------------------------------
Say "step 5/6: commit + tag $Tag"
if ($DryRun) {
    Say "  (dry-run) would: git add + commit + tag + push --follow-tags"
} else {
    git add Cargo.toml CHANGELOG.md
    git commit -m "release: $Tag"
    if ($LASTEXITCODE -ne 0) { Die "git commit failed" }
    git tag -a $Tag -m "Mighty $Tag"
    git push --follow-tags
    if ($LASTEXITCODE -ne 0) { Die "git push failed" }
}

# ----------------------------------------------------------------
# Step 6 — bundle + sign publishable packages.
# ----------------------------------------------------------------
Say "step 6/6: bundle + sign publishable packages"
if ($DryRun) {
    Say "  (dry-run) would: cargo build -p mty-cli + mty pkg publish (each pkg root)"
} else {
    cargo build -q -p mty-cli
    Say "  (v0.9) marketplace upload not enabled. Run 'mty pkg publish' in any package root for a signed bundle."
}

Say "done — released $Tag"
