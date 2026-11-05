"""Check the share package holds only what a colleague needs and refuses forbidden words."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]


class PackageTests(unittest.TestCase):
    def package(self, *arguments):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.output = Path(self.folder.name) / "share.zip"
        return subprocess.run([sys.executable, str(ROOT / "scripts/package.py"), "--output", str(self.output), *arguments],
                              capture_output=True, text=True)

    def test_package_holds_the_workspace_scripts_tests_and_readme_only(self):
        result = self.package()
        self.assertEqual(result.returncode, 0, result.stderr)
        names = zipfile.ZipFile(self.output).namelist()
        for needed in ("errand/README.md", "errand/LICENSE", "errand/docs/guide.md", "errand/docs/images/tasks.png", "errand/Cargo.toml", "errand/Cargo.lock", "errand/crates/errand-app/src/main.rs",
                       "errand/crates/errand-macos/tests/widget.rs", "errand/scripts/build.sh", "errand/scripts/install.py"):
            self.assertIn(needed, names)
        for name in names:
            top = name.split("/")[1]
            self.assertIn(top, {"README.md", "CHANGELOG.md", "LICENSE", "docs", "Cargo.toml", "Cargo.lock", "crates", "scripts", "Tests"}, name)
            self.assertNotIn("__pycache__", name)

    def test_forbidden_word_stops_packaging(self):
        result = self.package("--forbid", "SessionScanner")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("forbidden word", result.stderr)
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
