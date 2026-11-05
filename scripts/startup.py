"""Manage only the Errand login launcher, and retire the one Dual Codex left."""
from pathlib import Path
import os
import plistlib
import subprocess
import tempfile

LABEL = "com.errand.login"
LEGACY_LABEL = "com.dualcodex.login"

def configuration(app):
    return {"Label": LABEL, "ProgramArguments": ["/usr/bin/open", str(app)], "RunAtLoad": True}

def write_configuration(folder, app):
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    target = folder / (LABEL + ".plist")
    if target.is_symlink():
        raise ValueError(f"Linked login launcher preserved: {target}")
    if target.exists():
        existing = plistlib.loads(target.read_bytes())
        if existing.get("Label") != LABEL:
            raise ValueError(f"Unrelated login launcher preserved: {target}")
    data = plistlib.dumps(configuration(app))
    if not target.exists() or target.read_bytes() != data:
        descriptor, temporary = tempfile.mkstemp(prefix=".errand-", dir=folder)
        try:
            with os.fdopen(descriptor, "wb") as stream:
                stream.write(data)
            os.replace(temporary, target)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)
    target.chmod(0o600)
    return target

def legacy_configuration(folder):
    """The Dual Codex launcher's file, only when it really is that launcher."""
    target = Path(folder) / (LEGACY_LABEL + ".plist")
    if target.is_symlink() or not target.is_file():
        return None
    return target if plistlib.loads(target.read_bytes()).get("Label") == LEGACY_LABEL else None

def enable(app):
    folder = Path.home() / "Library/LaunchAgents"
    target = write_configuration(folder, app)
    legacy = legacy_configuration(folder)
    if legacy:
        subprocess.run(["/bin/launchctl", "bootout", f"gui/{os.getuid()}/{LEGACY_LABEL}"], capture_output=True)
        legacy.unlink()
        print(f"Retired the Dual Codex login launcher: {legacy}")
    service = f"gui/{os.getuid()}/{LABEL}"
    subprocess.run(["/bin/launchctl", "enable", service], check=True, capture_output=True)
    status = subprocess.run(["/bin/launchctl", "print", service], capture_output=True)
    if status.returncode != 0:
        subprocess.run(["/bin/launchctl", "bootstrap", f"gui/{os.getuid()}", str(target)], check=True, capture_output=True)
    subprocess.run(["/bin/launchctl", "print", service], check=True, capture_output=True)
    print(f"Login startup enabled: {target}")
