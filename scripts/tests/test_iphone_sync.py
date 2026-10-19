import importlib.util
from contextlib import redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("sync_iphone", ROOT / "scripts/sync_iphone.py")
sync = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sync)


class ExportTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="nano sync test ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.config = self.root / "config"
        self.config.mkdir()
        self.output = self.root / "export"
        self.audio = self.root / "歌曲.wav"
        self.audio.write_bytes((ROOT / "tests/fixtures/quiet.wav").read_bytes())
        self.lyrics = {"plain": ["Saved sidecar line"], "timed": []}
        metadata = dict(path=str(self.audio), title="Music", artist="Artist", album="Album", genre="",
                        number=1, duration=1.0, artwork=[1, 2, 3, 4], lyrics=self.lyrics)
        self.database = self.config / "library.sqlite3"
        with sqlite3.connect(self.database) as connection:
            connection.executescript("""
                CREATE TABLE tracks(path TEXT PRIMARY KEY, modified INTEGER, metadata BLOB);
                CREATE TABLE playlists(id INTEGER PRIMARY KEY,name TEXT);
                CREATE TABLE playlist_tracks(playlist_id INTEGER,position INTEGER,path TEXT);
                CREATE TABLE lyrics_override(audio_path TEXT,lyrics BLOB,removed INTEGER);
                CREATE TABLE track_ratings(path TEXT,stars INTEGER);
                CREATE TABLE playback_statistics(path TEXT,added_at INTEGER,play_count INTEGER,last_played INTEGER);
                CREATE TABLE listening_history(id INTEGER,path TEXT,played_at INTEGER,listened_seconds INTEGER);
            """)
            connection.execute("INSERT INTO tracks VALUES(?,0,?)", (str(self.audio), json.dumps(metadata).encode()))
            connection.execute("INSERT INTO playlists VALUES(7,'Dinner')")
            connection.execute("INSERT INTO playlist_tracks VALUES(7,0,?)", (str(self.audio),))
            connection.execute("INSERT INTO track_ratings VALUES(?,5)", (str(self.audio),))
            connection.execute("INSERT INTO playback_statistics VALUES(?,10,2,20)", (str(self.audio),))
            connection.execute("INSERT INTO listening_history VALUES(9,?,20,60)", (str(self.audio),))
        (self.config / "settings.json").write_text('{"language":"simplified_chinese"}')
        (self.config / "deepseek.json").write_text('{"api_key":"secret-must-not-be-exported"}')

    def export(self):
        return sync.export_package(self.config, self.output, progress=lambda _: None)

    def test_portable_package_preserves_order_sidecar_ratings_history_and_excludes_secrets(self):
        before = self.database.read_bytes()
        manifest = self.export()
        track = manifest["tracks"][0]
        self.assertEqual(sync.digest_file(self.output / track["file"]), track["sha256"])
        self.assertEqual(manifest["playlists"][0]["tracks"], [track["id"]])
        self.assertEqual(manifest["lyrics"][0]["lyrics"], self.lyrics)
        self.assertEqual(manifest["ratings"], [{"track": track["id"], "stars": 5}])
        self.assertEqual(manifest["history"][0]["id"], 9)
        self.assertEqual((self.output / track["artwork_file"]).read_bytes(), bytes([1, 2, 3, 4]))
        raw = (self.output / "manifest.json").read_text()
        self.assertNotIn(str(self.audio), raw)
        self.assertNotIn("api_key", raw)
        self.assertFalse((self.output / "deepseek.json").exists())
        self.assertFalse((self.output / "settings.json").exists())
        self.assertFalse((self.output / "READY").exists(), "copy must finish before commit")
        self.assertEqual(self.database.read_bytes(), before)

    def test_explicit_lyric_removal_wins_over_old_cached_sidecar_and_ids_are_stable(self):
        with sqlite3.connect(self.database) as connection:
            connection.execute("INSERT INTO lyrics_override VALUES(?,NULL,1)", (str(self.audio),))
        first = self.export()
        self.assertEqual(first["lyrics"], [{"track": first["tracks"][0]["id"], "removed": True}])
        second = self.export()
        self.assertEqual(first["export_id"], second["export_id"])
        self.assertEqual(first["tracks"][0]["id"], second["tracks"][0]["id"])

    def test_committed_output_cannot_be_mutated(self):
        self.export()
        (self.output / "READY").write_text("already committed")
        with self.assertRaisesRegex(ValueError, "fresh directory"):
            self.export()

    def test_usb_backup_precedes_copy_and_ready_is_published_last(self):
        manifest = self.export()
        actions = []

        class FakeDevice:
            def stop_app(self, directory):
                actions.append("stop")

            def copy_to(self, source, destination):
                if Path(source).name == "READY":
                    self.ready = Path(source).read_text()
                    actions.append("ready")
                elif (Path(source) / "READY").is_file():
                    self.invalid = (Path(source) / "READY").read_text()
                    self.cleared_report = json.loads((Path(source) / "report.json").read_text())
                    actions.append("invalidate")
                else:
                    actions.append("copy")

            def copy_from(self, source, destination):
                if source == "Documents/.nano":
                    actions.append("backup")
                elif source.endswith("receipt.json"):
                    sync.write_json(destination, dict(complete=True, export_id=self.ready, tracks=1, playlists=1, conflicts=0))
                else:
                    raise RuntimeError("file not available")

            def launch(self):
                actions.append("launch")

        device = FakeDevice()
        receipt = sync.deliver_package(device, self.output, self.root / "backup", wait_seconds=1, progress=lambda _: None)
        self.assertEqual(actions, ["stop", "backup", "invalidate", "copy", "ready", "launch"])
        self.assertEqual(receipt["export_id"], manifest["export_id"])
        self.assertNotEqual(device.invalid, manifest["export_id"])
        self.assertEqual(device.cleared_report["export_id"], manifest["export_id"])
        self.assertIsNone(device.cleared_report["error"])

    def test_copy_failure_relaunches_app_without_publishing_ready(self):
        self.export()
        actions = []

        class FailedDevice:
            def stop_app(self, directory):
                actions.append("stop")

            def copy_from(self, source, destination):
                actions.append("backup")

            def copy_to(self, source, destination):
                actions.append("copy")
                raise RuntimeError("USB disconnected")

            def launch(self):
                actions.append("launch")

        with self.assertRaisesRegex(RuntimeError, "USB disconnected"):
            sync.deliver_package(FailedDevice(), self.output, self.root / "backup", progress=lambda _: None)
        self.assertEqual(actions, ["stop", "backup", "copy", "launch"])

    def test_launch_device_option_precedes_the_variadic_app_arguments(self):
        import subprocess
        with patch.object(sync.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "", "")) as run:
            sync.Device("test-phone").launch()
        command = run.call_args[0][0]
        self.assertEqual(command[:5], ["xcrun", "devicectl", "device", "process", "launch"])
        self.assertLess(command.index("--device"), command.index(sync.BUNDLE))
        self.assertEqual(command[-1], sync.BUNDLE)

    def add_track(self, name, playlist=None):
        audio = self.root / name
        audio.write_bytes(self.audio.read_bytes())
        with sqlite3.connect(self.database) as connection:
            metadata = json.loads(connection.execute("SELECT metadata FROM tracks LIMIT 1").fetchone()[0])
            metadata.update(path=str(audio), title=name)
            connection.execute("INSERT INTO tracks VALUES(?,0,?)", (str(audio), json.dumps(metadata).encode()))
            if playlist is not None:
                connection.execute("INSERT INTO playlists VALUES(?,?)", (playlist, "Playlist " + str(playlist)))
                connection.execute("INSERT INTO playlist_tracks VALUES(?,0,?)", (playlist, str(audio)))
        return audio

    def cli(self, *arguments):
        stdout, stderr = io.StringIO(), io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            status = sync.main(["--json-events", "--config-dir", str(self.config), *arguments])
        events = [json.loads(line) for line in stdout.getvalue().splitlines()]
        return status, events, stderr.getvalue()

    def test_playlist_scope_includes_only_members_preserves_order_and_stable_ids(self):
        self.add_track("other.wav", playlist=8)
        with sqlite3.connect(self.database) as connection:
            connection.execute("INSERT INTO playlist_tracks VALUES(7,1,?)", (str(self.audio),))
        full = self.export()
        selected = sync.export_package(self.config, self.root / "selected", progress=lambda _: None,
                                       scope="playlists", playlist_ids=[7, 7])
        self.assertEqual(selected["source_id"], full["source_id"])
        self.assertEqual(len(selected["tracks"]), 1)
        track = selected["tracks"][0]
        self.assertEqual(track["id"], sync.identity(self.audio))
        self.assertEqual(selected["playlists"], [{"id": 7, "name": "Dinner", "tracks": [track["id"], track["id"]]}])
        self.assertEqual({path.name for path in (self.root / "selected/Music").iterdir()}, {track["id"]})
        self.assertTrue((self.root / "selected" / track["artwork_file"]).is_file())

    def test_all_scope_can_select_playlists_without_omitting_other_audio(self):
        self.add_track("other.wav", playlist=8)
        manifest = sync.export_package(self.config, self.output, progress=lambda _: None, playlist_ids=[7])
        self.assertEqual(len(manifest["tracks"]), 2)
        self.assertEqual([p["id"] for p in manifest["playlists"]], [7])

    def test_auxiliary_switches_remove_cached_lyrics_and_statistics_but_keep_audio_art(self):
        with sqlite3.connect(self.database) as connection:
            connection.execute("CREATE TABLE lyric_romaji(audio_path TEXT,annotation BLOB)")
            connection.execute("INSERT INTO lyric_romaji VALUES(?,?)", (str(self.audio), '{"lines":[]}'))
        (self.config / "translations.json").write_text(json.dumps({"entries": {"one": {
            "key": {"track_id": str(self.audio), "target": "zh", "provider_version": "test", "source": "line"},
            "lines": ["translated"]}}}))
        included = self.export()
        self.assertEqual(len(included["romaji"]), 1)
        self.assertEqual(len(included["translations"]), 1)
        excluded = sync.export_package(self.config, self.root / "excluded", progress=lambda _: None,
                                       include_lyrics=False, include_statistics=False)
        for section in ("lyrics", "romaji", "translations", "ratings", "statistics", "history"):
            self.assertEqual(excluded[section], [], section)
        self.assertEqual(excluded["tracks"][0]["lyrics"], {"plain": [], "timed": []})
        self.assertEqual(excluded["tracks"][0]["id"], included["tracks"][0]["id"])
        self.assertEqual(excluded["tracks"][0]["sha256"], included["tracks"][0]["sha256"])
        self.assertTrue((self.root / "excluded" / excluded["tracks"][0]["artwork_file"]).is_file())

    def test_changed_selection_requires_fresh_directory_to_avoid_transferring_old_payloads(self):
        self.export()
        with self.assertRaisesRegex(ValueError, "fresh output"):
            sync.export_package(self.config, self.output, progress=lambda _: None, include_lyrics=False)

    def test_missing_or_empty_playlist_selection_fails_before_payload_export(self):
        for ids in (None, [], [999]):
            with self.subTest(ids=ids), self.assertRaises(ValueError):
                sync.export_package(self.config, self.output, progress=lambda _: None,
                                    scope="playlists", playlist_ids=ids)
        self.assertFalse((self.output / "Music").exists())

    def test_plan_and_inspect_are_read_only_and_do_not_require_mac_or_phone(self):
        missing = self.add_track("missing.wav", playlist=8)
        missing.unlink()
        before = self.database.read_bytes()
        with patch.object(sync, "list_devices", side_effect=AssertionError("preview must not access devices")):
            status, events, stderr = self.cli("--inspect-library", "--output", str(self.output))
            self.assertEqual((status, stderr), (0, ""))
            self.assertEqual(len(events), 1)
            library = events[0]["library"]
            self.assertEqual((library["tracks"], library["available_tracks"], library["unavailable_tracks"]), (2, 1, 1))
            self.assertEqual(library["audio_bytes"], self.audio.stat().st_size)
            status, events, stderr = self.cli("--plan", "--scope", "playlists", "--playlist-id", "7",
                                               "--no-include-lyrics", "--no-include-statistics")
        self.assertEqual(status, 0)
        self.assertEqual(events[0]["plan"]["tracks"], 1)
        self.assertFalse(events[0]["plan"]["include_lyrics"])
        self.assertFalse(events[0]["plan"]["include_statistics"])
        self.assertNotIn(str(self.audio), json.dumps(events))
        self.assertFalse(self.output.exists())
        self.assertEqual(self.database.read_bytes(), before)

    def test_export_json_is_ndjson_with_real_track_progress_and_no_human_chatter(self):
        self.add_track("other.wav")
        status, events, stderr = self.cli("--output", str(self.output))
        self.assertEqual((status, stderr), (0, ""))
        progress = [e for e in events if e["phase"] == "export"]
        self.assertEqual([e["current"] for e in progress], [0, 1, 2])
        self.assertEqual([e["progress"] for e in progress], [0, 0.5, 1])
        self.assertTrue(all(e["total"] == 2 for e in progress))
        self.assertEqual(events[-1]["phase"], "exported")
        self.assertFalse((self.output / "READY").exists())

    def test_json_arguments_and_errors_do_not_expose_tool_output_or_credentials(self):
        status, events, stderr = self.cli("--unknown-argument")
        self.assertEqual(status, 1)
        self.assertEqual(events[0]["code"], "invalid_arguments")
        with patch.object(sync, "list_devices", side_effect=RuntimeError("api_key=secret-must-not-be-exported")):
            status, events, stderr = self.cli("--list-devices")
        self.assertEqual(status, 1)
        self.assertEqual(events[0]["phase"], "error")
        self.assertNotIn("secret", stderr + json.dumps(events))

    def test_json_help_is_also_one_json_event(self):
        status, events, stderr = self.cli("--help")
        self.assertEqual((status, stderr), (0, ""))
        self.assertEqual([event["phase"] for event in events], ["help"])
        self.assertIn("--cancel-file", events[0]["message"])

    def test_cli_cancellation_before_export_makes_no_package_or_device_change(self):
        cancel = self.root / "cancel"
        cancel.touch()
        with patch.object(sync, "list_devices", side_effect=AssertionError("must not contact devices")):
            status, events, stderr = self.cli("--output", str(self.output), "--cancel-file", str(cancel))
        self.assertEqual(status, 130)
        self.assertEqual(events[-1]["phase"], "cancelled")
        self.assertFalse((self.output / "manifest.json").exists())

    def test_copy_cancellation_invalidates_old_ready_and_restarts_without_commit(self):
        self.export()
        actions, events = [], []
        cancelled = False

        class FakeDevice:
            def stop_app(self, directory): actions.append("stop")
            def copy_from(self, source, destination): actions.append("backup")
            def copy_to(self, source, destination):
                nonlocal cancelled
                if (Path(source) / "READY").is_file():
                    self.marker = (Path(source) / "READY").read_text()
                    actions.append("invalidate")
                elif Path(source).is_dir():
                    cancelled = True
                    actions.append("copy")
                else:
                    self.marker = Path(source).read_text()
                    actions.append("commit")
            def launch(self): actions.append("launch")

        device = FakeDevice()
        with self.assertRaises(sync.Cancelled):
            sync.deliver_package(device, self.output, self.root / "backup", progress=lambda _: None,
                                 events=events.append, cancel=lambda: cancelled)
        self.assertEqual(actions, ["stop", "backup", "invalidate", "copy", "launch"])
        self.assertNotEqual(device.marker, self.export()["export_id"])
        self.assertNotIn("ready", [e["phase"] for e in events])
        self.assertTrue(all(e["progress"] is None for e in events))

    def test_cancel_at_ready_boundary_is_ignored_until_matching_receipt(self):
        manifest = self.export()
        actions, events = [], []
        cancelled = False

        class FakeDevice:
            def stop_app(self, directory): actions.append("stop")
            def copy_from(self, source, destination):
                if source.endswith("receipt.json"):
                    sync.write_json(destination, dict(complete=True, source_id=manifest["source_id"],
                                                     export_id=manifest["export_id"], tracks=1, conflicts=2))
                else:
                    actions.append("backup")
            def copy_to(self, source, destination):
                actions.append("commit" if Path(source).name == "READY" else "copy")
            def launch(self): actions.append("launch")

        def event(value):
            nonlocal cancelled
            events.append(value)
            if value["phase"] == "ready":
                self.assertFalse(value["cancellable"])
                cancelled = True

        receipt = sync.deliver_package(FakeDevice(), self.output, self.root / "backup", wait_seconds=1,
                                       progress=lambda _: None, events=event, cancel=lambda: cancelled)
        self.assertEqual(receipt["conflicts"], 2)
        self.assertEqual(events[-1]["receipt"], receipt)
        self.assertEqual([e["phase"] for e in events], ["backup", "copy", "ready", "importing", "complete"])
        self.assertEqual(actions[-2:], ["commit", "launch"])
        self.assertTrue(all(e["progress"] is None for e in events))

    def test_delivery_rejects_local_ready_before_touching_phone(self):
        self.export()
        (self.output / "READY").write_text("old")
        with patch.object(sync.Device, "stop_app") as stop:
            with self.assertRaisesRegex(ValueError, "local READY"):
                sync.deliver_package(sync.Device("unused"), self.output, self.root / "backup")
        stop.assert_not_called()

    def test_cancelled_real_device_adapter_disables_cancellation_during_recovery_launch(self):
        self.export()
        device = sync.Device("unused")
        was_cancelled = False
        old_cancel = lambda: False
        device.cancel = old_cancel
        launched = []

        def copy_to(source, destination):
            nonlocal was_cancelled
            if (Path(source) / "manifest.json").is_file():
                was_cancelled = True
            sync.check_cancel(device.cancel)

        def launch():
            self.assertIsNone(device.cancel)
            launched.append(True)

        with patch.object(device, "stop_app"), patch.object(device, "copy_from"), \
                patch.object(device, "copy_to", side_effect=copy_to), patch.object(device, "launch", side_effect=launch):
            with self.assertRaises(sync.Cancelled):
                sync.deliver_package(device, self.output, self.root / "backup", progress=lambda _: None,
                                     cancel=lambda: was_cancelled)
        self.assertEqual(launched, [True])
        self.assertIs(device.cancel, old_cancel)

    def test_stale_success_receipt_is_ignored_until_current_export_completes(self):
        manifest = self.export()
        reads = 0

        class FakeDevice:
            def stop_app(self, directory): pass
            def copy_to(self, source, destination): pass
            def launch(self): pass
            def copy_from(self, source, destination):
                nonlocal reads
                if source.endswith("receipt.json"):
                    reads += 1
                    sync.write_json(destination, dict(complete=True,
                        export_id="f" * 64 if reads == 1 else manifest["export_id"]))
                elif source.endswith("report.json"):
                    sync.write_json(destination, dict(export_id=manifest["export_id"], error=None))

        with patch.object(sync.time, "sleep"):
            receipt = sync.deliver_package(FakeDevice(), self.output, self.root / "backup", wait_seconds=1,
                                           progress=lambda _: None)
        self.assertEqual(reads, 2)
        self.assertEqual(receipt["export_id"], manifest["export_id"])

    def test_cancelled_cross_volume_audio_copy_cleans_partial_file(self):
        source = self.root / "large.wav"
        source.write_bytes(bytes(3 * 1024 * 1024))
        destination = self.root / "copied.wav"
        calls = 0
        def cancel():
            nonlocal calls
            calls += 1
            return calls > 1
        with self.assertRaises(sync.Cancelled):
            sync.copy_audio(source, destination, cancel)
        self.assertFalse(destination.exists())
        self.assertFalse(destination.with_name(destination.name + ".copying").exists())
        self.assertEqual(source.stat().st_size, 3 * 1024 * 1024)


class DeviceTests(unittest.TestCase):
    def test_discovery_filters_disconnected_wireless_unpaired_and_non_ios_and_limits_fields(self):
        def device(identifier, **overrides):
            item = dict(identifier="internal-" + identifier,
                        connectionProperties=dict(transportType="wired", tunnelState="connected", pairingState="paired"),
                        hardwareProperties=dict(udid=identifier, platform="iOS", deviceType="iPhone", serialNumber="private"),
                        deviceProperties=dict(name="Phone " + identifier))
            for section, value in overrides.items(): item[section].update(value)
            return item
        data = {"result": {"devices": [device("A"), device("B"),
            device("wireless", connectionProperties=dict(transportType="localNetwork")),
            device("offline", connectionProperties=dict(tunnelState="disconnected")),
            device("unpaired", connectionProperties=dict(pairingState="unpaired")),
            device("watch", hardwareProperties=dict(platform="watchOS", deviceType="AppleWatch"))]}}
        self.assertEqual(sync.connected_devices(data), [{"id": "A", "name": "Phone A"}, {"id": "B", "name": "Phone B"}])

    def test_discovery_supports_modern_devicectl_properties(self):
        data = {"result": {"devices": [{"identifier": "fallback", "properties": {
            "connection": {"transportType": "wired", "state": "connected", "pairingState": "paired"},
            "hardware": {"udid": "modern-phone", "platform": "iOS", "deviceType": "iPhone"},
            "state": {"name": "Modern phone"}}}]}}
        self.assertEqual(sync.connected_devices(data), [{"id": "modern-phone", "name": "Modern phone"}])

    def test_platform_and_missing_xcode_are_reported_without_device_commands(self):
        with patch.object(sync.sys, "platform", "linux"), patch.object(sync.subprocess, "run") as run:
            with self.assertRaisesRegex(sync.SyncError, "macOS"):
                sync.list_devices()
        run.assert_not_called()
        with patch.object(sync.sys, "platform", "darwin"), patch.object(sync.shutil, "which", return_value=None):
            with self.assertRaisesRegex(sync.SyncError, "Xcode"):
                sync.list_devices()

    def test_subprocess_errors_do_not_expose_arbitrary_device_diagnostics(self):
        with patch.object(sync.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, "api_key=private", "secret")):
            with self.assertRaises(sync.SyncError) as error:
                sync.Device("unused").launch()
        self.assertNotIn("secret", str(error.exception))
        self.assertNotIn("api_key", str(error.exception))

    def test_cancellable_subprocess_is_reaped_before_returning(self):
        processes = []
        original = sync.subprocess.Popen
        def spawn(*args, **kwargs):
            process = original(*args, **kwargs)
            processes.append(process)
            return process
        calls = 0
        def cancel():
            nonlocal calls
            calls += 1
            return calls >= 3
        with patch.object(sync.subprocess, "Popen", side_effect=spawn):
            with self.assertRaises(sync.Cancelled):
                sync.run_cancellable([sys.executable, "-c", "import time; time.sleep(30)"], 60, cancel)
        self.assertEqual(len(processes), 1)
        self.assertIsNotNone(processes[0].poll())


if __name__ == "__main__":
    unittest.main()
