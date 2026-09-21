#!/usr/bin/env python3
"""Replay an ANSI stream and print the final frame.

A raw capture concatenates every redraw, so a string being present says
nothing about what was on screen at the end -- which is how a footer that had
already been replaced kept showing up in earlier debugging. This keeps a
character buffer and applies enough of the escape sequences (cursor moves,
erase line, erase display) to answer "what does the user see".

    cols=140 rows=32 screen.py < capture
"""
import os, re, sys

ROWS = int(os.environ.get("rows", 32))
COLS = int(os.environ.get("cols", 140))
buf = [[" "] * COLS for _ in range(ROWS)]
cy = cx = 0


def clamp():
    global cy, cx
    cy = max(0, min(ROWS - 1, cy))
    cx = max(0, min(COLS - 1, cx))


data = sys.stdin.buffer.read().decode("utf-8", "replace")
i = 0
while i < len(data):
    ch = data[i]
    if ch == "\x1b":
        m = re.match(r"\x1b\[([0-9;?]*)([a-zA-Z])", data[i:])
        if not m:
            i += 1
            continue
        args = [int(x) for x in m.group(1).split(";") if x.isdigit()]
        fn = m.group(2)
        if fn == "H":
            cy = (args[0] - 1) if args else 0
            cx = (args[1] - 1) if len(args) > 1 else 0
        elif fn in "AB":
            cy += (args[0] if args else 1) * (1 if fn == "B" else -1)
        elif fn in "CD":
            cx += (args[0] if args else 1) * (1 if fn == "C" else -1)
        elif fn == "G":
            cx = (args[0] - 1) if args else 0
        elif fn == "J":
            mode = args[0] if args else 0
            if mode == 2:
                buf = [[" "] * COLS for _ in range(ROWS)]
            elif mode == 0:
                for col in range(cx, COLS):
                    buf[cy][col] = " "
                for row in range(cy + 1, ROWS):
                    buf[row] = [" "] * COLS
        elif fn == "K":
            mode = args[0] if args else 0
            if mode == 0:
                for col in range(cx, COLS):
                    buf[cy][col] = " "
            elif mode == 2:
                buf[cy] = [" "] * COLS
        clamp()
        i += m.end()
        continue
    if ch == "\r":
        cx = 0
    elif ch == "\n":
        cy += 1
        clamp()
    elif ch == "\b":
        cx = max(0, cx - 1)
    elif ch >= " ":
        clamp()
        buf[cy][cx] = ch
        cx += 1
    i += 1

for row in buf:
    line = "".join(row).rstrip()
    if line:
        print(line)
