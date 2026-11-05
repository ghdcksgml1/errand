"""Install Errand, retire the Dual Codex, Codex A and Codex B launchers, and preserve account data."""
from pathlib import Path
import os
import shutil
import uuid
import sys
import startup

source = Path(__file__).resolve().parents[1] / "dist"
destination = Path.home() / "Applications"
support = Path.home() / "Library/Application Support"
storage = support / "Errand"
names = ["Errand"]
update = "--update" in sys.argv


def is_ours(app):
    # Any earlier build counts, under either name and whatever bundle identifier; other apps are never replaced.
    return any((app / "Contents/MacOS" / executable).is_file() for executable in ("Errand", "DualCodex"))


# Launchers earlier builds installed beside the app, which now does their work itself.
retired = [destination / "Codex A.app", destination / "Codex B.app"]
legacy = destination / "Dual Codex.app"
for old in (*retired, legacy):
    if old.exists():
        if not is_ours(old):
            if old in retired:
                raise SystemExit(f"Different application; preserved: {old}")
            continue
        if not update:
            raise SystemExit(f"Already exists; preserved: {old}. Use --update to retire this launcher.")
for name in names:
    if not (source / f"{name}.app" / "Contents" / "MacOS" / "Errand").is_file():
        raise SystemExit(f"Build first: missing {name}.app")
    existing = destination / f"{name}.app"
    if existing.exists():
        if not update:
            raise SystemExit(f"Already exists; preserved: {existing}")
        if not is_ours(existing):
            raise SystemExit(f"Different application; preserved: {existing}")

# The move the app makes on its first launch, made here first so retired launchers are archived beside the rest.
# Account B's records name the old folder, so it stays as a link.
legacy_storage = support / "Dual Codex"
if legacy_storage.is_dir() and not legacy_storage.is_symlink() and not os.path.lexists(storage):
    legacy_storage.rename(storage)
    legacy_storage.symlink_to(storage.name)
    print(f"Moved app data to: {storage}")

destination.mkdir(parents=True, exist_ok=True)
for name in names:
    target = destination / f"{name}.app"
    temporary = destination / f".{name}-{uuid.uuid4()}.app"
    backup = destination / f".{name}-backup-{uuid.uuid4()}.app"
    try:
        shutil.copytree(source / f"{name}.app", temporary, symlinks=True)
        if target.exists():
            target.rename(backup)
        temporary.rename(target)
    except BaseException:
        if backup.exists() and not target.exists():
            backup.rename(target)
        if temporary.exists():
            shutil.rmtree(temporary)
        raise
    if backup.exists():
        shutil.rmtree(backup)
    print(f"Installed: {target}")

if legacy.exists() and is_ours(legacy):
    shutil.rmtree(legacy)
    print(f"Removed the Dual Codex launcher, replaced by: {destination / 'Errand.app'}")

for old in retired:
    if old.exists():
        archive = storage / "retired-launchers"
        archive.mkdir(parents=True, exist_ok=True)
        old.rename(archive / f"{old.stem}-{uuid.uuid4()}.app")
        print(f"Archived the {old.stem} launcher in: {archive}")

# A Dual Codex login launcher would open an app that is gone, so it moves to Errand even without --autostart.
if "--autostart" in sys.argv or startup.legacy_configuration(Path.home() / "Library/LaunchAgents"):
    startup.enable(destination / "Errand.app")
