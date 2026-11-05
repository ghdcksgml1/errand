"""Opt-in macOS integration check; restarts only the installed switcher."""
from pathlib import Path
import subprocess
import time

app = Path.home() / "Applications/Errand.app"
subprocess.run(["/usr/bin/open", str(app)], check=True)
time.sleep(1)
process = subprocess.Popen([str(app / "Contents/MacOS/Errand"), "--replace-running"],
                           stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, start_new_session=True)
deadline = time.monotonic() + 7
while time.monotonic() < deadline:
    if process.poll() is not None:
        raise SystemExit("FAIL: the replacement switcher exited instead of reopening")
    time.sleep(0.1)
print("PASS: the installed replacement switcher remains running")
