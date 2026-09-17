"""Exercise the real binary through a controlling PTY, without driving Ghostty."""
import errno
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import sys
import tempfile
import termios
import time

binary, repo = sys.argv[1:]


def run(mode):
    with tempfile.TemporaryFile(dir=Path(__file__).resolve().parent.parent / "target") as feedback:
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(repo)
            os.environ["TERM"] = "xterm-256color"
            os.dup2(feedback.fileno(), 1)
            os.execv(binary, [binary])
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        screen = bytearray()

        def read_until(text, timeout=20):
            deadline = time.monotonic() + timeout
            while text not in screen:
                assert time.monotonic() < deadline, (mode, text, bytes(screen[-2000:]))
                if select.select([fd], [], [], 0.1)[0]:
                    screen.extend(os.read(fd, 65536))

        try:
            read_until(b"working tree")
            if mode == "signal":
                os.kill(pid, signal.SIGTERM)
            else:
                os.write(fd, b"jc")
                read_until(b"Comment:")
                os.write(fd, b"\x1b[200~Please handle this case.\x1b[201~\r")
                read_until(b"Comment added")
                if mode == "stale":
                    Path(repo, "a.rs").write_text("changed during review\n")
                os.write(fd, b"q" if mode == "cancel" else b"S")
                read_until(b"Confirm")
                os.write(fd, b"y")
                if mode == "stale":
                    read_until(b"stale-snapshot")
                    os.write(fd, b"y")
            deadline = time.monotonic() + 20
            while True:
                assert time.monotonic() < deadline, (mode, "exit timeout", bytes(screen[-2000:]))
                if select.select([fd], [], [], 0.1)[0]:
                    try:
                        chunk = os.read(fd, 65536)
                    except OSError as error:
                        if error.errno != errno.EIO:
                            raise
                        break
                    if not chunk:
                        break
                    screen.extend(chunk)
            _, status = os.waitpid(pid, 0)
            code = os.waitstatus_to_exitcode(status)
            assert code == (2 if mode in ("cancel", "signal") else 0), (mode, code, screen[-2000:])
            feedback.seek(0)
            text = feedback.read()
            if mode in ("cancel", "signal"):
                assert text == b"", text
            else:
                assert b"Please handle this case." in text, text
                assert b'(old side, HEAD)' in text, text
                assert b"\x1b" not in text, text
                assert (b"WARNING:" in text) == (mode == "stale"), text
            assert b"\x1b[?1049l" in screen, "alternate screen was not restored"
            attrs = termios.tcgetattr(fd)
            assert attrs[3] & termios.ICANON, "terminal left in raw mode"
            assert attrs[3] & termios.ECHO, "terminal echo not restored"
        finally:
            try:
                os.kill(pid, signal.SIGKILL)
                os.waitpid(pid, 0)
            except (ProcessLookupError, ChildProcessError):
                pass
            os.close(fd)


for mode in ("send", "cancel", "signal", "stale"):
    run(mode)
    print(f"PTY {mode}: passed")
