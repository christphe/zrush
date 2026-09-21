# tests

```sh
./tests/run          # syntax, shellcheck, then every file
./tests/run 01       # only the files whose name starts with 01
```

Nothing here touches a real repository. `sandbox_create` builds a throwaway
repo with its own remote under `$TMPDIR`, points `XDG_CONFIG_HOME` at a config
whose editor and terminal are stubs that only append to a log, and
`sandbox_destroy` removes both the repo and the `~/.claude/projects/` entries
it created.

| file | what it covers | cost |
| --- | --- | --- |
| `01-data.sh` | the menu and the purge plan, no terminal involved | fast |
| `02-picker.sh` | what the screen actually shows, and which keys leave fzf | ~30 s |
| `03-wrappers.sh` | the eight scripts the picker generates for fzf to call back | ~10 s |

## lib

`drive.py` runs a program on a pty, sends keystrokes with delays, and returns
everything it wrote. It is not called `pty.py`, because a module of that name
beside the script shadows the standard library one it imports.

`screen.py` replays that stream and prints the **final frame**. A raw capture
concatenates every redraw, so grepping it tells you what was on screen at some
point, not what is there now — which is how a footer that had already been
replaced kept looking present.

## why these tests exist

Each one was written after a bug reached the user:

- an empty field in a tab-separated line collapsed and shifted every column
  after it, so deleting an orphan session silently did nothing
- `execute-silent` does not shield the terminal, so a command's output
  scribbled over the picker
- the reload wrapper re-ran the script without `--state`, so purge mode
  unfolded nothing
- a flag removed from the argument parser was still passed by a generated
  wrapper, so every `ctrl-d`, `ctrl-w` and `ctrl-p` failed without a word

None of them would have been caught by a syntax check. Three of the four are
now covered here.
