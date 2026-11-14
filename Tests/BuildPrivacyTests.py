"""Verify real compiler path remapping and the final bundle's privacy gate."""
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import build_privacy


class BuildPrivacyTests(unittest.TestCase):
    def test_encoded_flags_take_precedence_and_paths_with_spaces_reach_rustc_intact(self):
        with tempfile.TemporaryDirectory(prefix="errand privacy ") as folder:
            home = Path(folder) / "private user"
            source = home / "Project Name"
            source.mkdir(parents=True)
            code = source / "probe.rs"
            code.write_text('fn main() { println!("{}", file!()); }\n')
            executable = Path(folder) / "probe"
            env = build_privacy.build_environment({**os.environ, "RUSTFLAGS": "--invalid-flag-must-not-be-used",
                                                   "CARGO_ENCODED_RUSTFLAGS": "-C\x1fopt-level=0"}, source, home)
            flags = env["CARGO_ENCODED_RUSTFLAGS"].split(build_privacy.SEPARATOR)
            compiled = subprocess.run(["rustc", str(code), "-o", str(executable), *flags], capture_output=True, text=True)
            self.assertEqual(compiled.returncode, 0, compiled.stderr)
            self.assertEqual(subprocess.check_output([str(executable)], text=True).strip(), "/src/errand/probe.rs")
            self.assertEqual(build_privacy.private_files(executable, build_privacy.remappings(env, source, home)), [])

    def test_plain_rustflags_are_preserved_and_encoded_empty_flags_override_them(self):
        mappings = {"RUSTFLAGS": "-C opt-level=1 --cfg example"}
        flags = build_privacy.build_environment(mappings)["CARGO_ENCODED_RUSTFLAGS"].split(build_privacy.SEPARATOR)
        self.assertEqual(flags[:4], ["-C", "opt-level=1", "--cfg", "example"])
        flags = build_privacy.build_environment({**mappings, "CARGO_ENCODED_RUSTFLAGS": ""})["CARGO_ENCODED_RUSTFLAGS"]
        self.assertNotIn("opt-level=1", flags)

    def test_binary_gate_finds_private_paths_in_common_encodings_without_printing_them(self):
        with tempfile.TemporaryDirectory() as folder:
            app = Path(folder) / "Fixture.app"
            binary = app / "Contents/MacOS/Fixture"
            binary.parent.mkdir(parents=True)
            private = "/Users/private-fixture"
            for encoding in ["utf-8", "utf-16-le", "utf-16-be"]:
                binary.write_bytes(b"\xcf\xfa\xed\xfe\x00" + (private + "/.cargo/source.rs").encode(encoding))
                self.assertEqual(build_privacy.private_files(app, [(private, "/build/user")]), ["Contents/MacOS/Fixture"])
            binary.write_bytes(b"/build/cargo/source.rs\x00/src/errand/main.rs\x00/Users/person/example-project")
            self.assertEqual(build_privacy.private_files(app, [(private, "/build/user")]), [])

    def test_symlinked_checkout_and_custom_toolchain_homes_are_both_remapped(self):
        with tempfile.TemporaryDirectory() as folder:
            base = Path(folder)
            actual = base / "actual project"
            actual.mkdir()
            linked = base / "project alias"
            linked.symlink_to(actual, target_is_directory=True)
            env = {"CARGO_HOME": str(base / "cargo cache"), "RUSTUP_HOME": str(base / "rust tools")}
            maps = dict(build_privacy.remappings(env, linked, base / "private home"))
            self.assertEqual(maps[str(linked)], "/src/errand")
            self.assertEqual(maps[str(actual.resolve())], "/src/errand")
            self.assertEqual(maps[env["CARGO_HOME"]], "/build/cargo")
            self.assertEqual(maps[env["RUSTUP_HOME"]], "/build/rustup")

    def test_check_cli_fails_closed_on_a_leak_or_missing_bundle(self):
        with tempfile.TemporaryDirectory() as folder:
            target = Path(folder) / "binary"
            target.write_bytes(str(Path.home()).encode() + b"/.cargo/source.rs")
            result = subprocess.run([sys.executable, str(ROOT / "scripts/build_privacy.py"), "check", str(target)],
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("local build paths remain", result.stderr)
            self.assertNotIn(str(Path.home()), result.stderr)
            target.unlink()
            result = subprocess.run([sys.executable, str(ROOT / "scripts/build_privacy.py"), "check", str(target)],
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing", result.stderr)


if __name__ == "__main__":
    unittest.main()
