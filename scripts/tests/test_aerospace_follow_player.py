"""Exercise the optional hook without moving any real application windows."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


HOOK = Path(__file__).resolve().parents[1] / "aerospace-follow-player.sh"


class WorkspaceFollowTests(unittest.TestCase):
    def run_hook(self, windows="", workspace="2", failure=""):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            log = directory / "calls.jsonl"
            cli = directory / "aerospace"
            cli.write_text("""#!/usr/bin/env python3
import json, os, sys
with open(os.environ['NANO_TEST_LOG'], 'a') as log:
    log.write(json.dumps(sys.argv[1:]) + '\\n')
if sys.argv[1] == os.environ.get('NANO_TEST_FAILURE'):
    sys.exit(1)
if sys.argv[1] == 'list-windows':
    print(os.environ['NANO_TEST_WINDOWS'])
elif sys.argv[1] == 'list-workspaces':
    print(os.environ['NANO_TEST_WORKSPACE'])
""")
            cli.chmod(0o755)
            result = subprocess.run(
                ["/bin/bash", str(HOOK)],
                env={**os.environ, "NANO_AEROSPACE_BIN": str(cli),
                     "NANO_TEST_LOG": str(log), "NANO_TEST_WINDOWS": windows,
                     "NANO_TEST_WORKSPACE": workspace, "NANO_TEST_FAILURE": failure,
                     "AEROSPACE_FOCUSED_WORKSPACE": "stale-workspace"},
                capture_output=True, text=True, timeout=5,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "")
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            self.assertEqual(calls[0], ["list-windows", "--monitor", "focused",
                                      "--app-bundle-id", "org.nano.music-player",
                                      "--format", "%{window-id}|%{workspace}|%{window-layout}"])
            return [call for call in calls if call[0] == "move-node-to-workspace"]

    def test_closed_player_is_harmless(self):
        self.assertEqual(self.run_hook(), [])

    def test_only_floating_windows_off_current_workspace_move(self):
        self.assertEqual(self.run_hook("101|1|floating\n102|2|floating\n103|1|h_tiles"),
                         [["move-node-to-workspace", "--window-id", "101", "--", "2"]])

    def test_workspace_names_are_quoted_and_stale_event_is_ignored(self):
        self.assertEqual(self.run_hook("101|1|floating", "Music + Work"),
                         [["move-node-to-workspace", "--window-id", "101", "--", "Music + Work"]])

    def test_query_failures_do_not_move_windows(self):
        for failure in ["list-windows", "list-workspaces"]:
            self.assertEqual(self.run_hook("101|1|floating", failure=failure), [])

    def test_a_closed_window_does_not_prevent_moving_remaining_windows(self):
        self.assertEqual(len(self.run_hook("101|1|floating\n102|1|floating",
                                          failure="move-node-to-workspace")), 2)


if __name__ == "__main__":
    unittest.main()
