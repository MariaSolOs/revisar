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
target = Path(__file__).resolve().parent.parent / "target"
fake_bin = Path(tempfile.mkdtemp(dir=target))
nvim_log = fake_bin / "nvim.log"
# Stand-in nvim: records its arguments, writes to the terminal, edits the file.
Path(fake_bin, "nvim").write_text(
    "#!/bin/sh\n"
    f"printf '%s\\n' \"$PWD\" \"$@\" > '{nvim_log}'\n"
    "echo FAKE-NVIM-RAN\n"
    "printf 'edited\\n' > \"$3\"\n"
)
os.chmod(fake_bin / "nvim", 0o755)


def run(mode):
    with tempfile.TemporaryFile(dir=target) as feedback:
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(repo)
            os.environ["TERM"] = "xterm-256color"
            os.environ["PATH"] = f"{fake_bin}:{os.environ['PATH']}"
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
                if mode == "edit":
                    # The deleted "before" row opens at the line that replaced it.
                    os.write(fd, b"je")
                    read_until(b"Reloaded the diff after nvim")
                    assert b"FAKE-NVIM-RAN" in screen
                    root = Path(repo).resolve()
                    assert nvim_log.read_text() == f"{root}\n+1\n--\n{root / 'a.rs'}\n"
                    assert Path(repo, "a.rs").read_text() == "edited\n"
                os.write(fd, b"jc")
                read_until(b"Comment:")
                # Insert mode: Enter is a newline; Ctrl-s keeps the comment.
                os.write(fd, b"\x1b[200~Please handle\x1b[201~\r this case.\x13")
                read_until(b"Comment added")
                if mode == "stale":
                    Path(repo, "a.rs").write_text("changed during review\n")
                os.write(fd, b"q" if mode == "cancel" else b"S")
                if mode == "cancel":
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
                assert b"Please handle\n this case." in text, text
                if mode == "edit":
                    assert b"FAKE-NVIM-RAN" not in text, text
                    assert b"Reviewed code:\n```\nedited\n```" in text, text
                    assert b"(new side, working tree)" in text, text
                else:
                    assert b'(old side, base)' in text, text
                assert b"\x1b" not in text, text
                assert (b"WARNING:" in text) == (mode == "stale"), text
            if mode == "send":
                assert b"Confirm" not in screen, "send unexpectedly asked for confirmation"
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


for mode in ("send", "edit", "cancel", "signal", "stale"):
    run(mode)
    if mode == "edit":
        Path(repo, "a.rs").write_text("after\n")
    print(f"PTY {mode}: passed")
