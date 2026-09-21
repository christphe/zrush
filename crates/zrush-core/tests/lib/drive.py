#!/usr/bin/env python3
"""Drive a terminal program and return everything it wrote.

Not called pty.py on purpose: a module of that name next to the script
shadows the standard library one it imports.

Takes one JSON argument:

    {"cmd": [...], "steps": [[delay, keys], ...], "warmup": 5.0,
     "rows": 32, "cols": 140}

VEOF and VINTR are disabled on the pty, because ctrl-d and ctrl-c would
otherwise be eaten by the line discipline before the program ever sees them.
"""
import json, os, pty, select, signal, sys, time


def main():
    spec = json.loads(sys.argv[1])
    rows, cols = spec.get("rows", 32), spec.get("cols", 140)
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm"
        os.execvp(spec["cmd"][0], spec["cmd"])
        os._exit(1)
    try:
        import fcntl, struct, termios
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        attrs = termios.tcgetattr(fd)
        attrs[6][termios.VEOF] = b"\x00"
        attrs[6][termios.VINTR] = b"\x00"
        termios.tcsetattr(fd, termios.TCSANOW, attrs)
    except Exception:
        pass

    out = bytearray()

    def drain(seconds):
        end = time.time() + seconds
        while time.time() < end:
            ready, _, _ = select.select([fd], [], [], 0.05)
            if not ready:
                continue
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                return False
            if not chunk:
                return False
            out.extend(chunk)
        return True

    drain(spec.get("warmup", 5.0))
    for delay, keys in spec["steps"]:
        try:
            os.write(fd, keys.encode())
        except OSError:
            break
        if not drain(delay):
            break
    drain(1.0)

    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.kill(pid, sig)
        except Exception:
            break
        deadline = time.time() + 2
        reaped = False
        while time.time() < deadline:
            try:
                done, _ = os.waitpid(pid, os.WNOHANG)
            except Exception:
                reaped = True
                break
            if done:
                reaped = True
                break
            time.sleep(0.05)
        if reaped:
            break

    sys.stdout.write(bytes(out).decode(errors="replace"))


main()
