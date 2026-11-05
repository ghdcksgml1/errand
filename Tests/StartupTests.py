"""Check the login launcher without changing the user's login settings."""
from pathlib import Path
import importlib.util
import plistlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("startup", Path(__file__).resolve().parents[1] / "scripts" / "startup.py")
startup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(startup)

class StartupTests(unittest.TestCase):
    def test_login_opens_only_the_switcher(self):
        app = Path("/Users/test/Applications/Errand.app")
        config = startup.configuration(app)
        self.assertEqual(config.get("ProgramArguments"), ["/usr/bin/open", str(app)])
        self.assertTrue(config.get("RunAtLoad"))
        self.assertNotIn("KeepAlive", config)
        self.assertNotIn("StartInterval", config)

    def test_install_preserves_unrelated_files_and_can_repeat(self):
        with tempfile.TemporaryDirectory() as root:
            folder = Path(root) / "LaunchAgents"
            folder.mkdir()
            other = folder / "other.plist"
            other.write_bytes(b"user settings")
            app = Path(root) / "Applications/Errand.app"
            target = startup.write_configuration(folder, app)
            self.assertIsInstance(target, Path)
            self.assertEqual(plistlib.loads(target.read_bytes()), startup.configuration(app))
            self.assertEqual(startup.write_configuration(folder, app), target)
            self.assertEqual(other.read_bytes(), b"user settings")
            self.assertEqual(target.stat().st_mode & 0o777, 0o600)

    def test_unrelated_or_linked_target_is_never_replaced(self):
        with tempfile.TemporaryDirectory() as root:
            folder = Path(root)
            target = folder / (startup.LABEL + ".plist")
            target.write_bytes(plistlib.dumps({"Label":"someone.else"}))
            before = target.read_bytes()
            with self.assertRaises(ValueError):
                startup.write_configuration(folder, Path("/Applications/Errand.app"))
            self.assertEqual(target.read_bytes(), before)
            target.unlink()
            unrelated = folder / "unrelated.plist"
            unrelated.write_bytes(before)
            target.symlink_to(unrelated)
            with self.assertRaises(ValueError):
                startup.write_configuration(folder, Path("/Applications/Errand.app"))
            self.assertEqual(unrelated.read_bytes(), before)

    def test_only_the_real_dual_codex_launcher_is_retired(self):
        with tempfile.TemporaryDirectory() as root:
            folder = Path(root)
            self.assertIsNone(startup.legacy_configuration(folder))
            legacy = folder / (startup.LEGACY_LABEL + ".plist")
            legacy.write_bytes(plistlib.dumps({"Label": startup.LEGACY_LABEL}))
            self.assertEqual(startup.legacy_configuration(folder), legacy)
            legacy.write_bytes(plistlib.dumps({"Label": "someone.else"}))
            self.assertIsNone(startup.legacy_configuration(folder))
            legacy.unlink()
            other = folder / "other.plist"
            other.write_bytes(plistlib.dumps({"Label": startup.LEGACY_LABEL}))
            legacy.symlink_to(other)
            self.assertIsNone(startup.legacy_configuration(folder))

if __name__ == "__main__":
    unittest.main()
