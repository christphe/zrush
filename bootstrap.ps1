#
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Christophe Opoix
#
# bootstrap.sh for PowerShell: fetch a built zrush.exe and hand it to the
# install.ps1 that came with it. Nothing is compiled, so this needs neither
# Rust nor a checkout.
#
#   irm https://raw.githubusercontent.com/christphe/zrush/main/bootstrap.ps1 | iex
#   & ([scriptblock]::Create((irm .../bootstrap.ps1))) v0.0.2      that release
#   & ([scriptblock]::Create((irm .../bootstrap.ps1))) -Nightly    last green main, via gh
#
# Keep this file ASCII: Windows PowerShell 5.1 reads a file without a BOM
# in the ANSI code page.

# One block, so that piped into iex nothing here (variables, the error
# preference) leaks into the session that ran it; and errors are thrown,
# never exit, which would close that session's window.
& {
    param([string]$Version = "", [switch]$Nightly)
    $ErrorActionPreference = "Stop"
    $ProgressPreference = "SilentlyContinue"   # 5.1 crawls when it draws progress
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    $repo = "christphe/zrush"
    if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
        throw "no build for Windows on $env:PROCESSOR_ARCHITECTURE; build it yourself with cargo"
    }
    $target = "x86_64-pc-windows-msvc"
    $name = "zrush-$target"

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("zrush-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        if ($Nightly) {
            # Run artifacts are not public: only the GitHub CLI, already
            # authenticated, can reach them.
            if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw "gh is needed and not here" }
            $run = gh run list --repo $repo --workflow CI --branch main --status success `
                --limit 1 --json databaseId --jq '.[0].databaseId' 2>$null
            if (-not $run) { throw "no green run of CI on main to take a build from (or gh is not logged in)" }
            Write-Host "nightly: run $run, $target"
            gh run download $run --repo $repo -n $name -D $tmp
            if ($LASTEXITCODE -ne 0) { throw "that run has no $name artifact (they expire after 30 days)" }
            $root = $tmp
        } else {
            $tag = $Version
            if (-not $tag) {
                try {
                    $tag = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
                } catch {}
                if (-not $tag) { throw "no release published yet; try -Nightly" }
            }
            $file = "$name.zip"
            $url = "https://github.com/$repo/releases/download/$tag/$file"
            Write-Host "release $tag, $target"
            $zip = Join-Path $tmp $file
            try { Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zip }
            catch { throw "no such asset: $url" }

            # Best effort: a release published without checksums still
            # installs, a checksum that is there and disagrees stops everything.
            $sum = $null
            try { $sum = (Invoke-WebRequest -UseBasicParsing -Uri "$url.sha256").Content } catch {}
            if ($sum) {
                if ($sum -is [byte[]]) { $sum = [Text.Encoding]::ASCII.GetString($sum) }
                $want = ($sum.Trim() -split "\s+")[0].ToLower()
                $got = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash.ToLower()
                if ($got -ne $want) { throw "checksum mismatch: expected $want, got $got" }
                Write-Host "checksum ok"
            } else {
                Write-Host "note: no .sha256 alongside the archive; not verified"
            }

            Expand-Archive -LiteralPath $zip -DestinationPath $tmp -Force
            $root = Join-Path $tmp $name
        }

        $installer = Join-Path $root "install.ps1"
        if (-not (Test-Path -LiteralPath $installer)) {
            throw "no install.ps1 in what was downloaded (v0.0.1 shipped without one; see the README)"
        }
        # A script block, not a path: the execution policy forbids running a
        # .ps1 file on a stock Windows, and does not apply to this.
        & ([scriptblock]::Create([IO.File]::ReadAllText($installer))) -Root $root
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
} @args
