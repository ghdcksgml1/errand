"""Bundle what someone else needs to build Errand on their own Mac.

Usage: python3 scripts/package.py [--output PATH] [--forbid WORD ...]

Only the README with the docs, the Cargo workspace, scripts and tests go in. Build outputs, editor settings and
anything else in the folder stay out. With --forbid, packaging fails and no
archive is left if any packaged file contains one of the words, so names that must not leave the
company can be checked without writing them into the project.
"""
from pathlib import Path
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
INCLUDE = ["README.md", "CHANGELOG.md", "LICENSE", "docs", "Cargo.toml", "Cargo.lock", "crates", "scripts", "Tests"]
SKIP = {"__pycache__", ".DS_Store"}


def packaged_files():
    for entry in INCLUDE:
        path = ROOT / entry
        candidates = [path] if path.is_file() else sorted(path.rglob("*"))
        for file in candidates:
            relative = file.relative_to(ROOT)
            if file.is_file() and not SKIP.intersection(relative.parts) and file.suffix != ".pyc":
                yield file, relative


def main(arguments):
    forbidden = arguments[arguments.index("--forbid") + 1:] if "--forbid" in arguments else []
    output = Path(arguments[arguments.index("--output") + 1]) if "--output" in arguments else ROOT / "dist" / "Errand-source.zip"
    output.parent.mkdir(parents=True, exist_ok=True)
    found, count = [], 0
    with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
        for file, relative in packaged_files():
            contents = file.read_bytes()
            found += [f"{relative}: {word}" for word in forbidden if word.encode() in contents]
            archive.write(file, Path("errand") / relative)
            count += 1
    if found:
        output.unlink()
        raise SystemExit("Not packaged; these files contain a forbidden word:\n" + "\n".join(found))
    print(f"Packaged {count} files: {output}")


if __name__ == "__main__":
    main(sys.argv[1:])
