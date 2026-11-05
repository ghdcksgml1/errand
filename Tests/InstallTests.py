"""Exercise installation in a temporary HOME without touching real account data."""
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.support = self.home / "Library/Application Support"
        scripts = self.root / "scripts"
        scripts.mkdir()
        for name in ("install.py", "startup.py"):
            shutil.copy2(ROOT / "scripts" / name, scripts / name)
        self.bundle(self.root / "dist/Errand.app", "com.errand.app")

    def bundle(self, path, identifier, executable="Errand"):
        (path / "Contents/MacOS").mkdir(parents=True)
        (path / f"Contents/MacOS/{executable}").write_text("launcher fixture")
        (path / "Contents/Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": identifier}))

    def install(self):
        return subprocess.run([sys.executable, str(self.root / "scripts/install.py"), "--update"],
                              env={**os.environ, "HOME": str(self.home)}, capture_output=True, text=True)

    def test_update_retires_b_launcher_and_preserves_account_data(self):
        old = self.home / "Applications/Codex B.app"
        self.bundle(old, "com.example.dualcodex.b", executable="DualCodex")
        data = self.support / "Errand/accounts/b/home/sessions/keep.txt"
        data.parent.mkdir(parents=True)
        data.write_text("existing B conversation")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(old.exists())
        archived = list((self.support / "Errand/retired-launchers").glob("Codex B-*.app"))
        self.assertEqual(len(archived), 1)
        self.assertEqual((archived[0] / "Contents/MacOS/DualCodex").read_text(), "launcher fixture")
        self.assertEqual(data.read_text(), "existing B conversation")
        self.assertTrue((self.home / "Applications/Errand.app").exists())
        self.assertEqual(self.install().returncode, 0)

    def test_update_retires_the_codex_a_launcher(self):
        old = self.home / "Applications/Codex A.app"
        self.bundle(old, "com.errand.app.a")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(old.exists())
        archived = list((self.support / "Errand/retired-launchers").glob("Codex A-*.app"))
        self.assertEqual(len(archived), 1)
        self.assertEqual((archived[0] / "Contents/MacOS/Errand").read_text(), "launcher fixture")
        self.assertTrue((self.home / "Applications/Errand.app").exists())
        self.assertEqual(self.install().returncode, 0)

    def test_keeps_the_codex_a_launcher_without_update(self):
        old = self.home / "Applications/Codex A.app"
        self.bundle(old, "com.errand.app.a")
        result = subprocess.run([sys.executable, str(self.root / "scripts/install.py")],
                                env={**os.environ, "HOME": str(self.home)}, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Use --update to retire this launcher", result.stderr)
        self.assertTrue(old.exists())

    def test_update_moves_dual_codex_data_and_keeps_the_old_path_working(self):
        legacy = self.support / "Dual Codex"
        (legacy / "accounts/b/home/sessions").mkdir(parents=True)
        (legacy / "accounts/b/home/sessions/keep.txt").write_text("existing B conversation")
        (legacy / "session-tray.plist").write_text("checked cards")
        old = self.home / "Applications/Codex B.app"
        self.bundle(old, "com.example.dualcodex.b", executable="DualCodex")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        storage = self.support / "Errand"
        self.assertFalse(storage.is_symlink())
        self.assertEqual((storage / "session-tray.plist").read_text(), "checked cards")
        self.assertEqual((storage / "accounts/b/home/sessions/keep.txt").read_text(), "existing B conversation")
        self.assertEqual(len(list((storage / "retired-launchers").glob("Codex B-*.app"))), 1)
        # B's own records name the old folder.
        self.assertTrue(legacy.is_symlink())
        self.assertEqual((legacy / "accounts/b/home/sessions/keep.txt").read_text(), "existing B conversation")
        self.assertEqual(self.install().returncode, 0)
        self.assertTrue(legacy.is_symlink())

    def test_existing_errand_data_is_never_overwritten_by_the_old_folder(self):
        (self.support / "Dual Codex").mkdir(parents=True)
        (self.support / "Dual Codex/session-tray.plist").write_text("old")
        (self.support / "Errand").mkdir()
        (self.support / "Errand/session-tray.plist").write_text("new")
        self.assertEqual(self.install().returncode, 0)
        self.assertEqual((self.support / "Errand/session-tray.plist").read_text(), "new")
        self.assertEqual((self.support / "Dual Codex/session-tray.plist").read_text(), "old")

    def test_update_replaces_the_dual_codex_launcher_with_errand(self):
        legacy = self.home / "Applications/Dual Codex.app"
        self.bundle(legacy, "com.dualcodex.app", executable="DualCodex")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(legacy.exists())
        with (self.home / "Applications/Errand.app/Contents/Info.plist").open("rb") as file:
            self.assertEqual(plistlib.load(file)["CFBundleIdentifier"], "com.errand.app")

    def test_update_replaces_a_build_made_under_another_identifier(self):
        older = self.home / "Applications/Errand.app"
        self.bundle(older, "com.example.older.errand")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        with (older / "Contents/Info.plist").open("rb") as file:
            self.assertEqual(plistlib.load(file)["CFBundleIdentifier"], "com.errand.app")

    def test_preserves_unrelated_app_named_errand(self):
        other = self.home / "Applications/Errand.app"
        self.bundle(other, "example.unrelated", executable="Other")
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Different application; preserved", result.stderr)
        self.assertTrue((other / "Contents/MacOS/Other").exists())

    def test_leaves_unrelated_app_named_dual_codex_alone(self):
        other = self.home / "Applications/Dual Codex.app"
        self.bundle(other, "example.unrelated", executable="Other")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((other / "Contents/MacOS/Other").exists())

    def test_preserves_unrelated_app_named_codex_b(self):
        old = self.home / "Applications/Codex B.app"
        self.bundle(old, "example.unrelated", executable="Other")
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Different application; preserved", result.stderr)
        self.assertTrue(old.exists())

    def test_preserves_unrelated_app_named_codex_a(self):
        old = self.home / "Applications/Codex A.app"
        self.bundle(old, "example.unrelated", executable="Other")
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Different application; preserved", result.stderr)
        self.assertTrue(old.exists())


if __name__ == "__main__":
    unittest.main()
