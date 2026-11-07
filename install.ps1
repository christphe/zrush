#
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Christophe Opoix
#
# install.sh for PowerShell: put zrush.exe on your Path, with a config and,
# for Zed, a task and a keybinding. Nothing is built here: the binary is the
# one next to this script, which is how the Windows zip is laid out.
#
# Your editor's settings.json is never touched; the note at the end says
# what to add.
#
#   powershell -ExecutionPolicy Bypass -File install.ps1
#   powershell -ExecutionPolicy Bypass -File install.ps1 -Editor zed
#   ... -Bin path\zrush.exe     install that binary
#   ... -Yes                    add to the Path without asking
#
# Keep this file ASCII: Windows PowerShell 5.1 reads a file without a BOM
# in the ANSI code page.
param(
    [string]$Editor = "",
    [string]$Bin = "",
    [string]$Root = $PSScriptRoot,
    [switch]$Yes
)
$ErrorActionPreference = "Stop"

# The same places install.sh and the binary use: dirs::home_dir is
# USERPROFILE, and zrush reads ~/.config even on Windows.
$home_dir = $env:USERPROFILE
$bin_dir = Join-Path $home_dir ".local\bin"
$cfg_root = if ($env:XDG_CONFIG_HOME) { $env:XDG_CONFIG_HOME } else { Join-Path $home_dir ".config" }
$cfg_dir = Join-Path $cfg_root "zrush"
$cfg_file = Join-Path $cfg_dir "config.toml"
$zed_dir = Join-Path $env:APPDATA "Zed"

# A BOM at the head of config.toml is a parse error to zrush, and 5.1's
# -Encoding UTF8 writes one.
function Write-Utf8([string]$path, [string]$text) {
    [IO.File]::WriteAllText($path, $text, (New-Object Text.UTF8Encoding $false))
}

if (-not $Bin) {
    foreach ($c in @((Join-Path $Root "zrush.exe"), (Join-Path $Root "target\release\zrush.exe"))) {
        if (Test-Path -LiteralPath $c) { $Bin = $c; break }
    }
}
if (-not $Bin -or -not (Test-Path -LiteralPath $Bin)) {
    # throw, not exit: run by bootstrap.ps1 this shares the caller's
    # PowerShell, and exit would close the window.
    throw ("no zrush.exe found next to this script.`n`n" +
        "  from a checkout:  cargo build --release, then run this again`n" +
        "  from a release:   https://github.com/christphe/zrush/releases")
}

New-Item -ItemType Directory -Force -Path $bin_dir, $cfg_dir | Out-Null

# Windows will not overwrite an exe that is running, in an editor's terminal
# panel say, but it will rename one. The old copy goes aside and is cleaned
# up by the next install that finds it free.
$dst = Join-Path $bin_dir "zrush.exe"
try { Remove-Item -LiteralPath "$dst.old" -Force -ErrorAction Stop } catch {}
if (Test-Path -LiteralPath $dst) {
    try { Move-Item -LiteralPath $dst -Destination "$dst.old" -Force -ErrorAction Stop } catch {}
}
Copy-Item -LiteralPath $Bin -Destination $dst -Force
$version = try { (& $dst --version 2>$null) -join " " } catch { "unknown" }
Write-Host "installed $dst ($version)"

# The Path that new terminals and editors start with. Read and written in
# the registry rather than through SetEnvironmentVariable, which would store
# it expanded and turn every %USERPROFILE% in it into a fixed string.
$want = $bin_dir.TrimEnd("\")
$machine = [Environment]::GetEnvironmentVariable("Path", "Machine")
$user = [Environment]::GetEnvironmentVariable("Path", "User")
$entries = "$user;$machine" -split ";" | ForEach-Object { $_.TrimEnd("\") }
if ($entries -notcontains $want) {
    $add = $Yes
    if (-not $add) {
        $answer = Read-Host "$bin_dir is not on your Path; add it for your user? [Y/n]"
        $add = ($answer -eq "" -or $answer -match "^(y|yes|o|oui)$")
    }
    if ($add) {
        $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
        $old = $k.GetValue("Path", "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $new = if ($old) { $old.TrimEnd(";") + ";" + $bin_dir } else { $bin_dir }
        $k.SetValue("Path", $new, [Microsoft.Win32.RegistryValueKind]::ExpandString)
        $k.Close()
        # Setting any variable broadcasts the change, so new terminals see it.
        [Environment]::SetEnvironmentVariable("ZRUSH_PATH_PING", "1", "User")
        [Environment]::SetEnvironmentVariable("ZRUSH_PATH_PING", $null, "User")
        $env:Path = "$env:Path;$bin_dir"
        Write-Host "added $bin_dir to your Path; new terminals will find zrush"
    } else {
        Write-Host "warning: $bin_dir is not on your Path"
    }
}

# The old shell-syntax config is imported on first run and left in place, so
# there is nothing to do here for an existing setup.
if (-not (Test-Path -LiteralPath $cfg_file) -and -not (Test-Path -LiteralPath (Join-Path $cfg_dir "config"))) {
    Write-Utf8 $cfg_file @'
# ~/.config/zrush/config.toml. Never put secrets here.

# Repo used when zrush runs outside any git repository.
# default_repo = "C:/Users/you/projects/thing"

# Which coding agent to list. Unset means ask, unless only one is installed.
# agent = "claude"

# Editor that enter opens a worktree in: zed, cursor or code (run with -n).
# Also settable from the interface, with :editor.
# editor = "zed"
# editor_cmd = ["code", "--reuse-window"]   # or override the whole command

# How ctrl-o opens a terminal.
# terminal = ["wt", "-d", "."]

# titles = true          # read session titles from transcripts
# status = true          # the per-worktree git status column
# resumable_max = 5      # dead sessions listed per worktree
# resumable_scan = 40    # newest transcripts examined
# more_step = 20         # rows added by one [...more]
# title_width = 48       # truncate session titles here
# preview_turns = 14     # conversation turns in the preview pane

'@
    Write-Host "wrote $cfg_file"
}

if (-not $Editor) {
    $Editor = Read-Host "editor [zed/cursor/code, enter to skip]"
}

# Asking which editor and then not setting it is what the first version of
# install.sh did, and it left every install on the default.
if ($Editor) {
    $text = [IO.File]::ReadAllText($cfg_file)
    if ($text -match "(?m)^editor = ") {
        $text = $text -replace "(?m)^editor = .*$", "editor = `"$Editor`""
    } else {
        if (-not $text.EndsWith("`n")) { $text += "`n" }
        $text += "editor = `"$Editor`"`n"
    }
    Write-Utf8 $cfg_file $text
    Write-Host "editor = `"$Editor`" in $cfg_file"
}

if ($Editor -eq "zed") {
    $src = Join-Path $Root "zed"
    if (-not (Test-Path -LiteralPath $src)) {
        Write-Host "note: no zed\ next to this script; get tasks.json and keymap.json"
        Write-Host "      from https://github.com/christphe/zrush/tree/main/zed"
    } else {
        New-Item -ItemType Directory -Force -Path $zed_dir | Out-Null
        $skipped = @()
        foreach ($f in @("tasks.json", "keymap.json")) {
            $to = Join-Path $zed_dir $f
            if (Test-Path -LiteralPath $to) {
                Write-Host "note: $to exists; merge $(Join-Path $src $f) yourself"
                $skipped += $f
            } else {
                Copy-Item -LiteralPath (Join-Path $src $f) -Destination $to
                Write-Host "wrote $to"
            }
        }
        # The keybinding names a task by its label, so half an install leaves
        # a key bound to nothing.
        if ($skipped -contains "tasks.json" -and $skipped -notcontains "keymap.json") {
            Write-Host "note: the keybinding just written needs the task in $(Join-Path $src 'tasks.json')"
        }
    }
    Write-Host @'

One thing left, in your Zed settings.json, so the terminal panel resumes
this worktree's session:

  "terminal": { "shell": { "with_arguments": {
      "program": "zrush", "args": ["session"] } } }
'@
}

Write-Host ""
Write-Host "done. Run zrush from inside a git repository."
