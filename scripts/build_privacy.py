"""Remap local build paths and refuse app bundles that still contain them.

Used by build.sh for both native and universal builds. Cargo's encoded flags keep
paths containing spaces intact, and existing caller flags remain in effect.
"""
from pathlib import Path
import os
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SEPARATOR = "\x1f"


def remappings(environ, root=ROOT, home=None):
    home = Path(home) if home is not None else Path.home()
    locations = [
        (home, "/build/user"),
        (Path(environ.get("CARGO_HOME", home / ".cargo")), "/build/cargo"),
        (Path(environ.get("RUSTUP_HOME", home / ".rustup")), "/build/rustup"),
        (Path(root), "/src/errand"),
    ]
    result = []
    for source, replacement in locations:
        # A symlinked home or checkout can reach the compiler through either spelling.
        source = source.expanduser().absolute()
        for path in dict.fromkeys([source, source.resolve()]):
            if path == Path(path.anchor):
                raise ValueError("Refusing to remap the filesystem root")
            result.append((str(path), replacement))
    return result


def build_environment(environ, root=ROOT, home=None):
    env = dict(environ)
    encoded = env.get("CARGO_ENCODED_RUSTFLAGS")
    # Cargo gives encoded flags precedence; ordinary RUSTFLAGS split on whitespace.
    flags = encoded.split(SEPARATOR) if encoded else []
    if encoded is None:
        flags = env.get("RUSTFLAGS", "").split()
    # Last matching remap wins, so the workspace's specific prefix comes last.
    flags += [f"--remap-path-prefix={source}={target}" for source, target in remappings(env, root, home)]
    env["CARGO_ENCODED_RUSTFLAGS"] = SEPARATOR.join(flags)
    return env


def private_files(bundle, mappings):
    needles = {source.encode(encoding) for source, _ in mappings for encoding in ("utf-8", "utf-16-le", "utf-16-be")}
    files = [bundle] if bundle.is_file() else sorted(bundle.rglob("*"))
    found = []
    for file in files:
        if file.is_symlink() or not file.is_file():
            continue
        contents = file.read_bytes()
        if any(needle in contents for needle in needles):
            # Do not repeat the private path or matched contents in build logs.
            found.append(file.name if bundle.is_file() else str(file.relative_to(bundle)))
    return found


def main(arguments):
    if arguments and arguments[0] == "build":
        return subprocess.run(["cargo", "build", "--release", "--locked", *arguments[1:]],
                              cwd=ROOT, env=build_environment(os.environ)).returncode
    if len(arguments) == 2 and arguments[0] == "check":
        bundle = Path(arguments[1])
        if not bundle.exists():
            raise SystemExit("Privacy check failed: app bundle is missing")
        found = private_files(bundle, remappings(os.environ))
        if found:
            raise SystemExit("Privacy check failed: local build paths remain in " + ", ".join(found))
        print("Build privacy check passed: no local home, toolchain or source paths")
        return 0
    raise SystemExit("Usage: build_privacy.py build [cargo build flags] | check APP")


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
