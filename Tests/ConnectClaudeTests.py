"""Connect Claude Code's status line in a temporary HOME, without touching the person's own settings."""
from pathlib import Path
import json
import os
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ConnectClaudeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name) / "my home"
        self.executable = self.home / "Applications/Errand.app/Contents/MacOS/Errand"
        self.executable.parent.mkdir(parents=True)
        # Keeps what it was given, as the app keeps a status line's input.
        self.executable.write_text("#!/bin/sh\ncat > \"$0.input\"\n")
        self.executable.chmod(0o755)
        self.settings = self.home / ".claude/settings.json"
        self.wanted = f"'{self.executable}' --claude-status-line"

    def connect(self):
        return subprocess.run([sys.executable, str(ROOT / "scripts/connect_claude.py")],
                              env={**os.environ, "HOME": str(self.home)}, capture_output=True, text=True)

    def write(self, settings):
        self.settings.parent.mkdir(parents=True, exist_ok=True)
        self.settings.write_text(json.dumps(settings))

    def read(self):
        return json.loads(self.settings.read_text())

    def test_first_connection_makes_the_settings(self):
        result = self.connect()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.read(), {"statusLine": {"type": "command", "command": self.wanted}})
        self.assertEqual(Path(f"{self.executable}.input").read_text(), "{}", "the app hears from the status line at once, so its card stops asking")

    def test_the_command_runs_through_a_shell_from_a_path_with_a_space(self):
        self.executable.write_text("#!/bin/sh\necho \"ran $1\"\n")
        self.connect()
        command = self.read()["statusLine"]["command"]
        self.assertEqual(subprocess.run(command, shell=True, capture_output=True, text=True).stdout, "ran --claude-status-line\n")

    def test_everything_else_in_the_settings_stays_in_its_order(self):
        self.write({"zebra": 1, "model": "opus", "permissions": {"allow": ["Read"]}})
        self.settings.chmod(0o600)
        self.assertEqual(self.connect().returncode, 0)
        self.assertEqual(list(self.read()), ["zebra", "model", "permissions", "statusLine"])
        self.assertEqual(self.read()["permissions"], {"allow": ["Read"]})
        self.assertEqual(self.settings.stat().st_mode & 0o777, 0o600)

    def test_connecting_twice_changes_nothing(self):
        self.connect()
        before = self.settings.read_text()
        result = self.connect()
        self.assertEqual(result.returncode, 0)
        self.assertIn("Already connected", result.stdout)
        self.assertEqual(self.settings.read_text(), before)

    def test_an_earlier_installs_command_is_replaced(self):
        self.write({"statusLine": {"type": "command", "command": "/old/place/Errand --claude-status-line"}})
        self.assertEqual(self.connect().returncode, 0)
        self.assertEqual(self.read()["statusLine"]["command"], self.wanted)

    def test_someone_elses_status_line_is_preserved_with_a_way_to_share_it(self):
        mine = {"statusLine": {"type": "command", "command": "~/my-status.sh"}}
        self.write(mine)
        result = self.connect()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.read(), mine)
        self.assertIn("--claude-status-line", result.stderr)

    def test_settings_it_cannot_read_are_preserved(self):
        self.settings.parent.mkdir(parents=True)
        self.settings.write_text("{ not json")
        self.assertNotEqual(self.connect().returncode, 0)
        self.assertEqual(self.settings.read_text(), "{ not json")

    def test_a_linked_settings_file_stays_linked(self):
        kept = self.home / "dotfiles/claude.json"
        kept.parent.mkdir(parents=True)
        kept.write_text("{}")
        self.settings.parent.mkdir(parents=True)
        self.settings.symlink_to(kept)
        self.assertEqual(self.connect().returncode, 0)
        self.assertTrue(self.settings.is_symlink())
        self.assertIn("statusLine", json.loads(kept.read_text()))

    def test_an_errand_without_the_mode_is_not_connected(self):
        self.executable.write_text("#!/bin/sh\nexit 3\n")
        result = self.connect()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Update Errand first", result.stderr)
        self.assertFalse(self.settings.exists())

    def test_an_errand_that_opens_instead_of_exiting_is_not_left_running_or_connected(self):
        self.executable.write_text("#!/bin/sh\nexec sleep 60\n")
        result = self.connect()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Update Errand first", result.stderr)
        self.assertFalse(self.settings.exists())

    def test_nothing_is_connected_before_the_app_is_installed(self):
        self.executable.unlink()
        result = self.connect()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Install first", result.stderr)
        self.assertFalse(self.settings.exists())


if __name__ == "__main__":
    unittest.main()
