"""Point Claude Code's status line at Errand, the documented way for it to share its usage limits."""
from pathlib import Path
import json
import os
import shlex
import shutil
import subprocess

FLAG = "--claude-status-line"


def command(app):
    return f"{shlex.quote(str(app / 'Contents/MacOS/Errand'))} {FLAG}"


def connect(app):
    path = Path.home() / ".claude/settings.json"
    executable = app / "Contents/MacOS/Errand"
    if not executable.is_file():
        raise SystemExit(f"Install first: missing {app}")
    try:
        settings = json.loads(path.read_text()) if path.exists() else {}
    except ValueError:
        settings = None
    if not isinstance(settings, dict):
        raise SystemExit(f"Not readable as settings; preserved: {path}")
    current = settings.get("statusLine")
    wanted = command(app)
    # An earlier install's command is ours to replace; anyone else's status line is the person's own.
    ours = isinstance(current, dict) and str(current.get("command", "")).endswith(FLAG)
    if current is not None and not ours:
        raise SystemExit(
            f"Claude Code already has a status line; preserved: {path}\n"
            f"Pipe its input to Errand too, at the start of that command:\n"
            f"  input=$(cat); printf '%s' \"$input\" | {wanted}"
        )
    # The record this makes is how the app knows the status line reaches it, so the card stops asking to connect
    # before Claude Code's first run. An Errand older than this mode would open instead of exiting.
    try:
        subprocess.run([str(executable), FLAG], input=b"{}", check=True, timeout=5, capture_output=True)
    except (OSError, subprocess.SubprocessError):
        raise SystemExit(f"Update Errand first, this one can't take a status line: {app}")
    if ours and current.get("command") == wanted:
        print(f"Already connected: {path}")
        return
    settings["statusLine"] = {"type": "command", "command": wanted}
    # A settings file kept elsewhere stays a link to it.
    target = path.resolve()
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_name(f".{target.name}.{os.getpid()}")
    temporary.write_text(json.dumps(settings, indent=2, ensure_ascii=False) + "\n")
    if target.exists():
        shutil.copymode(target, temporary)
    temporary.replace(target)
    print(f"Connected Claude Code's status line: {path}")


if __name__ == "__main__":
    connect(Path.home() / "Applications/Errand.app")
