#!/usr/bin/env bash
#
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Christophe Opoix
#
# LICENSE as the RTF the Windows installer shows on its licence page. Made
# at build time from LICENSE, so the two never drift apart.
#
#   tools/windows/license-rtf.sh > target/License.rtf
#
# Lines of a paragraph are joined so the text wraps to the dialog's width
# rather than at LICENSE's 72 columns; a blank line ends a paragraph.
set -euo pipefail
cd "$(dirname "$0")/../.."

awk '
BEGIN {
    printf "{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fswiss Segoe UI;}}\\viewkind4\\uc1\\pard\\f0\\fs17 "
}
function flush() {
    if (para != "") { printf "%s\\par\\par\n", para; para = "" }
}
{
    sub(/\r$/, "")
    if ($0 ~ /^[ \t]*$/) { flush(); next }
    line = $0
    gsub(/\\/, "\\\\", line)
    gsub(/\{/, "\\{", line)
    gsub(/\}/, "\\}", line)
    sub(/^[ \t]+/, "", line)
    para = (para == "") ? line : para " " line
}
END { flush(); print "}" }
' LICENSE
