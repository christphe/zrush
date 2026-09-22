#!/bin/sh
#
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Christophe Opoix
#
# Fetch a built zrush and hand it to the install.sh that came with it.
# Nothing is compiled, so this needs neither Rust nor a checkout.
#
#   curl -fsSL https://raw.githubusercontent.com/christphe/zrush/main/bootstrap.sh | sh
#   sh bootstrap.sh v0.1.0      that release rather than the latest
#   sh bootstrap.sh --nightly   the last green build of main, via gh
#
# Piped from curl there is no one to answer the editor question, so it is
# skipped; run ~/.local/bin/zrush after this, or install.sh from a
# checkout, to set that part up.
set -eu

REPO=christphe/zrush
want=""
nightly=0

for arg in "$@"; do
  case "$arg" in
    --nightly) nightly=1 ;;
    -h|--help)
      # A heredoc, not a slice of this file: piped from curl there is no
      # file to slice.
      cat <<'USAGE'
zrush bootstrap — fetch a built binary and install it.

  sh bootstrap.sh              the latest release
  sh bootstrap.sh v0.1.0       that release
  sh bootstrap.sh --nightly    the last green build of main, via gh
USAGE
      exit 0
      ;;
    *) want="$arg" ;;
  esac
done

die() { echo "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is needed and not here"; }

# The five targets CI builds. Windows is a zip to unpack by hand: there is
# no ~/.local/bin to install into there anyway.
os=$(uname -s)
arch=$(uname -m)
case "$os/$arch" in
  Darwin/arm64)          target=aarch64-apple-darwin ;;
  Darwin/x86_64)         target=x86_64-apple-darwin ;;
  Linux/x86_64)          target=x86_64-unknown-linux-gnu ;;
  Linux/aarch64|Linux/arm64) target=aarch64-unknown-linux-gnu ;;
  *) die "no build for $os/$arch; build it yourself with ./install-dev.sh" ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if [ "$nightly" -eq 1 ]; then
  # Run artifacts are not public: only the GitHub CLI, already authenticated,
  # can reach them.
  need gh
  run=$(gh run list --repo "$REPO" --workflow CI --branch main \
        --status success --limit 1 --json databaseId \
        --jq '.[0].databaseId' 2>/dev/null) || die "gh could not list the runs; try: gh auth login"
  [ -n "$run" ] || die "no green run of CI on main to take a build from"
  echo "nightly: run $run, $target"
  gh run download "$run" --repo "$REPO" -n "zrush-$target" -D "$tmp" \
    || die "that run has no zrush-$target artifact (they expire after 30 days)"
  root="$tmp"
else
  need curl
  need tar
  tag="$want"
  if [ -z "$tag" ]; then
    # 2>/dev/null: no release yet is a 404, and curl's own complaint about
    # it says less than the line below.
    tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
          | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
    [ -n "$tag" ] || die "no release published yet; try --nightly"
  fi
  name="zrush-$target"
  url="https://github.com/$REPO/releases/download/$tag/$name.tar.gz"
  echo "release $tag, $target"
  curl -fsSL -o "$tmp/$name.tar.gz" "$url" || die "no such asset: $url"

  # Best effort: a release published without checksums still installs, a
  # checksum that is there and disagrees stops everything.
  if curl -fsSL -o "$tmp/$name.tar.gz.sha256" "$url.sha256" 2>/dev/null; then
    want_sum=$(awk '{print $1}' "$tmp/$name.tar.gz.sha256")
    if command -v sha256sum >/dev/null 2>&1; then
      got=$(sha256sum "$tmp/$name.tar.gz" | awk '{print $1}')
    else
      got=$(shasum -a 256 "$tmp/$name.tar.gz" | awk '{print $1}')
    fi
    [ "$got" = "$want_sum" ] || die "checksum mismatch: expected $want_sum, got $got"
    echo "checksum ok"
  else
    echo "note: no .sha256 alongside the archive; not verified"
  fi

  tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
  root="$tmp/$name"
fi

[ -f "$root/install.sh" ] || die "no install.sh in what was downloaded"
# A zipped artifact loses the executable bit on the way out of GitHub.
chmod +x "$root/install.sh" "$root/zrush" 2>/dev/null || true
"$root/install.sh"
