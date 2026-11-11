# Builds the release players download: mcc-rust-rewrite-beta-<version>.zip with the exe, the
# licence texts, the third-party notices, the README and the changelog.
#
#   & scripts\release.ps1            builds, checks and zips into out\release\
#   & scripts\release.ps1 -NoBuild   zips the release exe already built
#
# The exe is the release profile (no debug info, no symbols), built with the paths of this
# PC (the cargo and rustup folders, this checkout) mapped away, and the script fails if the
# exe still names the Windows user or the checkout. Nothing a player saves goes in the zip.
# The version is the workspace's (Cargo.toml `[workspace.package] version`).
param(
    [string]$Out,
    [switch]$NoBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not $Out) { $Out = Join-Path $root 'out\release' }
$pkg = 'mcc-rust-rewrite'

function Say($m) { Write-Host "[release $(Get-Date -f HH:mm:ss)] $m" }

Push-Location $root
try {
    # --locked is not passed here: this only reads, and the build below is the check.
    $meta = cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-msvc | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
    $version = ($meta.packages | Where-Object { $_.name -eq $pkg }).version
    $name = "$pkg-beta-$version"
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { $meta.target_directory }
    $exe = Join-Path $target "release\$pkg.exe"

    if (-not $NoBuild) {
        $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
        $rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
        # Separated by 0x1F, since the paths may hold spaces.
        $flags = @(
            "--remap-path-prefix=$cargoHome=cargo",
            "--remap-path-prefix=$rustupHome=rustup",
            "--remap-path-prefix=$root=."
        )
        $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]0x1f
        Say "building $name"
        cargo build --release --locked -p $pkg
        $code = $LASTEXITCODE
        Remove-Item Env:\CARGO_ENCODED_RUSTFLAGS
        if ($code -ne 0) { throw "cargo build failed (exit $code)" }
    }
    if (-not (Test-Path $exe)) { throw "no release exe at $exe" }

    # The exe must not name this PC: the user, or where the checkout is.
    $bytes = [System.IO.File]::ReadAllBytes($exe)
    $latin = [System.Text.Encoding]::GetEncoding(28591).GetString($bytes)
    $wide = [System.Text.Encoding]::Unicode.GetString($bytes)
    $private = @($env:USERNAME, $root, $root.Replace('\', '/')) | Where-Object { $_ -and $_.Length -ge 4 } | Select-Object -Unique
    foreach ($p in $private) {
        foreach ($text in $latin, $wide) {
            $at = $text.IndexOf($p, [System.StringComparison]::OrdinalIgnoreCase)
            if ($at -ge 0) {
                $from = [Math]::Max(0, $at - 60)
                $near = $text.Substring($from, [Math]::Min(160, $text.Length - $from)) -replace '[^\x20-\x7e]', '.'
                throw "the exe names this PC ('$p'): ...$near..."
            }
        }
    }
    Say 'the exe names neither the user nor the checkout'

    $stage = Join-Path $Out $name
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item $exe $stage
    foreach ($f in 'LICENSE', 'LICENSE-APACHE', 'README.md', 'CHANGELOG.md') { Copy-Item (Join-Path $root $f) $stage }

    @"
MCC Rust Rewrite $("beta-$version")

Halo maps from your own copy of Halo: The Master Chief Collection, with Skate 3 skating and
the MX vs ATV Reflex dirt bike.

To play: run mcc-rust-rewrite.exe. The first run asks for the folder of your MCC install
(needed), and of Skate 3 and MX vs ATV Reflex if you have them (both optional). No game
files come with this download; it reads your own.

Your settings and the converted game data are kept in %LOCALAPPDATA%\MccRustRewrite, with
the logs in its logs folder. Settings > Support opens that folder and copies a summary to
paste into a bug report.

README.md says more; CHANGELOG.md lists what changed.

Licence: GNU General Public License version 3 only (LICENSE). The source code is at
https://github.com/TheProcessorCodes/mcc-rust-rewrite
A few files are adapted from Apache-2.0 code (LICENSE-APACHE). The libraries built into the
exe and their licences are in THIRD-PARTY-NOTICES.txt.
"@ | Set-Content (Join-Path $stage 'READ ME FIRST.txt') -Encoding utf8

    # The libraries in the exe: every package of the build that is not ours, with the
    # licence files its source carries. The same text is printed once, under every
    # package that ships it.
    $ours = @($meta.workspace_members)
    $deps = $meta.packages | Where-Object { $ours -notcontains $_.id -and $_.source } | Sort-Object name, version
    # Ours by path, but under their own licences.
    $vendored = $meta.packages | Where-Object { -not $_.source -and $_.manifest_path -like '*\third_party\*' } | Sort-Object name
    $texts = [ordered]@{}
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add("MCC Rust Rewrite beta-$version is built with the libraries below. Each is listed with its")
    $lines.Add('licence; the licence texts their sources carry follow the list.')
    $lines.Add('')
    foreach ($p in @($vendored) + @($deps)) {
        $licence = if ($p.license) { $p.license } elseif ($p.license_file) { "see $($p.license_file)" } else { 'not stated' }
        $lines.Add("$($p.name) $($p.version)  [$licence]  $($p.repository)")
        $dir = Split-Path $p.manifest_path -Parent
        Get-ChildItem $dir -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE|COPYRIGHT)' } | ForEach-Object {
            $text = (Get-Content $_.FullName -Raw)
            if (-not $text) { return }
            $key = ($text -replace '\s+', ' ').Trim()
            if (-not $texts.Contains($key)) { $texts[$key] = @{ Text = $text.TrimEnd(); Users = New-Object System.Collections.Generic.List[string] } }
            $texts[$key].Users.Add("$($p.name) $($p.version) ($($_.Name))")
        }
    }
    foreach ($t in $texts.Values) {
        $lines.Add('')
        $lines.Add('=' * 78)
        $lines.Add(($t.Users -join ', '))
        $lines.Add('=' * 78)
        $lines.Add($t.Text)
    }
    $lines | Set-Content (Join-Path $stage 'THIRD-PARTY-NOTICES.txt') -Encoding utf8
    Say "$(@($deps).Count + @($vendored).Count) libraries, $($texts.Count) licence texts in THIRD-PARTY-NOTICES.txt"

    $zip = Join-Path $Out "$name.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    Say "$zip ($([Math]::Round((Get-Item $zip).Length / 1MB, 1)) MB, exe $([Math]::Round((Get-Item $exe).Length / 1MB, 1)) MB)"
    Say "sha256 $hash"
} finally {
    Pop-Location
}
