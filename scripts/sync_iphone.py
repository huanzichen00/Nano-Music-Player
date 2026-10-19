#!/usr/bin/env python3
"""Export desktop music data and deliver a committed package over a paired USB device.

The iOS app merges the package using the shared Rust library on its next launch.
Desktop settings, credentials and original audio files are never modified.

GUI protocol: --json-events writes NDJSON events with phase, progress (0..1 or
null), current, total, message and cancellable. Library/device/plan/receipt
objects appear on their corresponding events. --cancel-file is cooperative:
creating the file or sending SIGTERM/SIGINT cancels before phase=ready. From
ready onward the worker must remain alive until completion/error; cancellation
is ignored so a phone merge can finish safely. Bulk USB copy has no percentage.
"""
import argparse
from contextlib import contextmanager
import hashlib
import json
import math
import os
import re
import signal
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time

BUNDLE = "com.huanzichen.nanomusicplayer.ios"


class SyncError(RuntimeError):
    """A diagnostic which is safe to display without subprocess output."""
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


class Cancelled(SyncError):
    def __init__(self):
        super().__init__("cancelled", "Sync cancelled before the phone import was committed")


def check_cancel(cancel):
    if cancel is not None and cancel():
        raise Cancelled()


def emit(events, phase, message, *, current=None, total=None, cancellable=True, **data):
    if events is not None:
        progress = current / total if current is not None and total else None
        events(dict(phase=phase, progress=progress, current=current, total=total,
                    message=message, cancellable=cancellable, **data))


@contextmanager
def library_snapshot(config, cancel=None):
    database = Path(config).resolve() / "library.sqlite3"
    if not database.is_file():
        raise ValueError("The selected desktop library.sqlite3 does not exist")
    with tempfile.TemporaryDirectory(prefix="nano-sync-snapshot-") as snapshot:
        source = sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)
        connection = sqlite3.connect(str(Path(snapshot) / "library.sqlite3"))
        try:
            source.backup(connection, pages=128, progress=lambda *_: check_cancel(cancel))
            connection.row_factory = sqlite3.Row
            yield connection
        finally:
            connection.close()
            source.close()


def selection(connection, scope="all", playlist_ids=None):
    if scope not in ("all", "playlists"):
        raise ValueError("Sync scope must be all or playlists")
    tables = {r[0] for r in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    playlists = []
    if "playlists" in tables:
        for row in connection.execute("SELECT id, name FROM playlists ORDER BY id"):
            members = [r[0] for r in connection.execute(
                "SELECT path FROM playlist_tracks WHERE playlist_id=? ORDER BY position", (row["id"],))]
            playlists.append(dict(id=row["id"], name=row["name"], paths=members))
    requested = None if playlist_ids is None else set(playlist_ids)
    if scope == "playlists" and not requested:
        raise ValueError("Select at least one playlist for playlist sync")
    if requested is not None:
        if requested - {p["id"] for p in playlists}:
            raise ValueError("A selected playlist no longer exists; refresh the desktop library")
        playlists = [p for p in playlists if p["id"] in requested]
    selected_paths = {path for p in playlists for path in p["paths"]} if scope == "playlists" else None
    return tables, playlists, selected_paths


def inspect_library(config, *, scope="all", playlist_ids=None,
                    include_lyrics=True, include_statistics=True, cancel=None):
    """Read a consistent, read-only plan without hashing or creating an export."""
    check_cancel(cancel)
    with library_snapshot(config, cancel) as connection:
        _, playlists, selected_paths = selection(connection, scope, playlist_ids)
        available = set()
        count, audio_bytes = 0, 0
        for row in connection.execute("SELECT path FROM tracks ORDER BY path"):
            check_cancel(cancel)
            path = row[0]
            if selected_paths is not None and path not in selected_paths:
                continue
            count += 1
            audio = Path(path)
            try:
                if audio.is_file():
                    audio_bytes += audio.stat().st_size
                    available.add(path)
            except OSError:
                pass
        return dict(source_id=identity(Path(config).resolve()), scope=scope, tracks=count,
                    available_tracks=len(available), unavailable_tracks=count-len(available),
                    audio_bytes=audio_bytes, include_lyrics=include_lyrics,
                    include_statistics=include_statistics, playlists=[dict(
                        id=p["id"], name=p["name"], tracks=len(p["paths"]),
                        available_tracks=sum(path in available for path in p["paths"])) for p in playlists])


def digest_file(path, cancel=None):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            check_cancel(cancel)
            digest.update(chunk)
    return digest.hexdigest()


def copy_audio(source, destination, cancel):
    temporary = destination.with_name(destination.name + ".copying")
    try:
        with source.open("rb") as reader, temporary.open("wb") as writer:
            for chunk in iter(lambda: reader.read(1024 * 1024), b""):
                check_cancel(cancel)
                writer.write(chunk)
        shutil.copystat(source, temporary)
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)


def identity(value):
    return hashlib.sha256(str(value).encode("utf-8")).hexdigest()


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False), encoding="utf-8")
    temporary.replace(path)


def export_package(config, output, progress=print, *, scope="all", playlist_ids=None,
                   include_lyrics=True, include_statistics=True, events=None, cancel=None):
    config, output = Path(config).resolve(), Path(output).resolve()
    database = config / "library.sqlite3"
    if not database.is_file():
        raise ValueError("The selected desktop library.sqlite3 does not exist")
    output.mkdir(parents=True, exist_ok=True)
    if (output / "READY").exists():
        raise ValueError("Export into a fresh directory; an existing READY package must not be changed")
    manifest = dict(version=1, source_id=identity(config), export_id="", tracks=[], playlists=[],
                    lyrics=[], romaji=[], ratings=[], statistics=[], history=[], translations=[])
    warnings = []
    check_cancel(cancel)
    with library_snapshot(config, cancel) as connection:
        tables, playlists, selected_paths = selection(connection, scope, playlist_ids)
        options = dict(scope=scope, playlist_ids=None if playlist_ids is None else sorted(set(playlist_ids)),
                       include_lyrics=include_lyrics, include_statistics=include_statistics)
        summary_file = output / "export-summary.json"
        if summary_file.is_file():
            previous = json.loads(summary_file.read_text(encoding="utf-8"))
            if previous.get("options", dict(scope="all", playlist_ids=None,
                                            include_lyrics=True, include_statistics=True)) != options:
                raise ValueError("Choose a fresh output directory when changing sync selections")
        count = sum(1 for r in connection.execute("SELECT path FROM tracks")
                    if selected_paths is None or r[0] in selected_paths)
        emit(events, "export", "Preparing desktop music", current=0, total=count)
        paths = {}
        lyric_records = {}
        index = 0
        for row in connection.execute("SELECT path, metadata FROM tracks ORDER BY path"):
            check_cancel(cancel)
            if selected_paths is not None and row["path"] not in selected_paths:
                continue
            index += 1
            original = row["path"]
            audio = Path(original)
            if not audio.is_file():
                warnings.append("Skipped an unavailable desktop audio file")
                emit(events, "export", "Skipped an unavailable audio file", current=index, total=count)
                continue
            track_id = identity(original)
            paths[original] = track_id
            metadata = json.loads(row["metadata"])
            before = audio.stat()
            audio_hash = digest_file(audio, cancel)
            relative = Path("Music") / track_id / audio.name
            destination = output / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            if destination.exists():
                if not os.path.samefile(audio, destination) and digest_file(destination, cancel) != audio_hash:
                    raise ValueError("An existing export contains different audio; choose a fresh output directory")
            else:
                try:
                    os.link(audio, destination)
                except OSError:
                    copy_audio(audio, destination, cancel)
            after = audio.stat()
            if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
                raise ValueError("An audio file changed during export; retry when tag editing has finished")
            duration = metadata.get("duration", 0.0)
            if not isinstance(duration, (float, int)) or not math.isfinite(duration) or duration < 0:
                raise ValueError("A cached track has an invalid duration")
            track = dict(id=track_id, file=relative.as_posix(), size=after.st_size, sha256=audio_hash,
                         title=metadata.get("title", ""), artist=metadata.get("artist", ""),
                         album=metadata.get("album", ""), genre=metadata.get("genre", ""),
                         number=metadata.get("number", 0), duration=duration,
                         lyrics=metadata.get("lyrics", {"plain": [], "timed": []}) if include_lyrics else {"plain": [], "timed": []})
            artwork = bytes(metadata.get("artwork", []))
            if artwork:
                relative_art = Path("Artwork") / (track_id + ".bin")
                art_file = output / relative_art
                art_file.parent.mkdir(parents=True, exist_ok=True)
                art_file.write_bytes(artwork)
                track.update(artwork_file=relative_art.as_posix(), artwork_size=len(artwork),
                             artwork_sha256=hashlib.sha256(artwork).hexdigest())
            manifest["tracks"].append(track)
            # Cached lyrics can come from a desktop LRC sidecar which is
            # outside the audio file. Preserve them as a durable association
            # so the first mobile rescan cannot discard those lines.
            if track["lyrics"].get("plain") or track["lyrics"].get("timed"):
                lyric_records[track_id] = dict(track=track_id, removed=False, lyrics=track["lyrics"])
            emit(events, "export", "Prepared desktop audio", current=index, total=count)
            if index % 10 == 0 or index == count:
                progress("Prepared {}/{} desktop tracks".format(index, count))
        for playlist in playlists:
            members = [paths[path] for path in playlist["paths"] if path in paths]
            manifest["playlists"].append(dict(id=playlist["id"], name=playlist["name"], tracks=members))
        if include_lyrics and "lyrics_override" in tables:
            for row in connection.execute("SELECT audio_path, lyrics, removed FROM lyrics_override"):
                if row["audio_path"] in paths:
                    entry = dict(track=paths[row["audio_path"]], removed=bool(row["removed"]))
                    if row["lyrics"] is not None and not entry["removed"]:
                        entry["lyrics"] = json.loads(row["lyrics"])
                    lyric_records[entry["track"]] = entry
        manifest["lyrics"] = list(lyric_records.values())
        if include_lyrics and "lyric_romaji" in tables:
            for row in connection.execute("SELECT audio_path, annotation FROM lyric_romaji"):
                if row["audio_path"] in paths:
                    manifest["romaji"].append(dict(track=paths[row["audio_path"]], annotation=json.loads(row["annotation"])))
        for table, section, columns in [
            ("track_ratings", "ratings", ("stars",)),
            ("playback_statistics", "statistics", ("added_at", "play_count", "last_played")),
            ("listening_history", "history", ("id", "played_at", "listened_seconds")),
        ]:
            if include_statistics and table in tables:
                for row in connection.execute("SELECT path," + ",".join(columns) + " FROM " + table):
                    if row["path"] in paths:
                        manifest[section].append(dict(track=paths[row["path"]], **{k: row[k] for k in columns}))
    translation_file = config / "translations.json"
    if include_lyrics and translation_file.is_file():
        cache = json.loads(translation_file.read_text(encoding="utf-8"))
        for entry in cache.get("entries", {}).values():
            key = entry["key"]
            if key["track_id"] in paths:
                manifest["translations"].append(dict(track=paths[key["track_id"]], target=key["target"],
                    provider_version=key["provider_version"], source=key["source"], lines=entry["lines"]))
    check_cancel(cancel)
    manifest["export_id"] = identity(json.dumps(manifest, sort_keys=True, ensure_ascii=False, allow_nan=False))
    write_json(output / "manifest.json", manifest)
    # READY stays out of the copied package until every file has reached iOS.
    write_json(output / "export-summary.json", dict(
        tracks=len(manifest["tracks"]), audio_bytes=sum(t["size"] for t in manifest["tracks"]),
        playlists=len(manifest["playlists"]), lyrics=len(manifest["lyrics"]), romaji=len(manifest["romaji"]),
        ratings=len(manifest["ratings"]), history=len(manifest["history"]), translations=len(manifest["translations"]),
        warnings=warnings, options=options))
    return manifest


def require_usb_tools():
    if sys.platform != "darwin":
        raise SyncError("unsupported_platform", "USB sync requires macOS and Xcode with devicectl")
    if shutil.which("xcrun") is None:
        raise SyncError("xcode_required", "Install Xcode and select its developer tools to enable USB sync")
    try:
        result = subprocess.run(["xcrun", "--find", "devicectl"], capture_output=True,
                                text=True, timeout=15, check=False)
    except (OSError, subprocess.TimeoutExpired):
        raise SyncError("xcode_required", "Xcode devicectl is unavailable; open Xcode and finish its setup") from None
    if result.returncode:
        raise SyncError("xcode_required", "Xcode devicectl is unavailable; open Xcode and finish its setup")


def connected_devices(data):
    """Allowlist the public fields of connected, paired USB iPhones/iPads."""
    result = []
    seen = set()
    if not isinstance(data, dict) or not isinstance(data.get("result"), dict):
        raise SyncError("device_discovery_failed", "Xcode returned an unreadable device list; retry discovery")
    items = data["result"].get("devices")
    if not isinstance(items, list):
        raise SyncError("device_discovery_failed", "Xcode returned an unreadable device list; retry discovery")
    for item in items:
        if not isinstance(item, dict):
            continue
        modern = item.get("properties", {})
        if not isinstance(modern, dict):
            modern = {}
        connection = item.get("connectionProperties") or modern.get("connection", {})
        hardware = item.get("hardwareProperties") or modern.get("hardware", {})
        properties = item.get("deviceProperties") or modern.get("state", {})
        if not all(isinstance(section, dict) for section in (connection, hardware, properties)):
            continue
        transport = str(connection.get("transportType", "")).lower()
        state = str(connection.get("tunnelState", connection.get("connectionState", connection.get("state", "")))).lower()
        paired = str(connection.get("pairingState", "paired")).lower()
        if transport not in ("wired", "usb") or (state != "connected" and connection.get("isConnected") is not True):
            continue
        if paired not in ("paired", "trusted"):
            continue
        if hardware.get("platform") not in ("iOS", "iPadOS"):
            continue
        if hardware.get("deviceType") not in ("iPhone", "iPad"):
            continue
        identifier = hardware.get("udid") or item.get("identifier")
        if not isinstance(identifier, str) or not identifier or identifier in seen:
            continue
        seen.add(identifier)
        name = properties.get("name")
        result.append(dict(id=identifier, name=name if isinstance(name, str) and name else "iPhone"))
    return sorted(result, key=lambda item: (item["name"].casefold(), item["id"]))


def list_devices():
    require_usb_tools()
    with tempfile.TemporaryDirectory(prefix="nano-sync-devices-") as temporary:
        record = Path(temporary) / "devices.json"
        try:
            result = subprocess.run(["xcrun", "devicectl", "list", "devices", "--json-output", str(record), "--quiet"],
                                    capture_output=True, text=True, timeout=30, check=False)
            if result.returncode:
                raise SyncError("device_discovery_failed", "Unable to list USB devices; unlock and trust the iPhone, then retry")
            return connected_devices(json.loads(record.read_text(encoding="utf-8")))
        except (OSError, json.JSONDecodeError, subprocess.TimeoutExpired):
            raise SyncError("device_discovery_failed", "Unable to list USB devices; unlock and trust the iPhone, then retry") from None


def run_cancellable(command, timeout, cancel):
    """Drain child pipes while polling cancellation; never leave devicectl copying."""
    check_cancel(cancel)
    with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as process:
        deadline = time.monotonic() + timeout
        try:
            while True:
                check_cancel(cancel)
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise subprocess.TimeoutExpired(command, timeout)
                try:
                    stdout, stderr = process.communicate(timeout=min(0.2, remaining))
                    return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
                except subprocess.TimeoutExpired:
                    continue
        except BaseException:
            process.terminate()
            try:
                process.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate()
            raise


class Device:
    def __init__(self, udid, cancel=None):
        self.udid = udid
        self.cancel = cancel

    def run(self, *arguments, timeout=120):
        # `process launch` treats anything after the bundle identifier as app
        # arguments, so target-device options must precede that positional value.
        command = ["xcrun", "devicectl", "device", *arguments[:2],
                   "--device", self.udid, *arguments[2:]]
        if self.cancel is None:
            result = subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
        else:
            result = run_cancellable(command, timeout, self.cancel)
        if result.returncode:
            # devicectl diagnostics may contain environment or credential data.
            raise SyncError("device_operation_failed", "USB device operation failed; unlock the iPhone and check its connection")
        return result

    def copy_to(self, source, destination, timeout=1800):
        return self.run("copy", "to", "--domain-type", "appDataContainer", "--domain-identifier", BUNDLE,
                        "--source", str(source), "--destination", destination, timeout=timeout)

    def copy_from(self, source, destination):
        return self.run("copy", "from", "--domain-type", "appDataContainer", "--domain-identifier", BUNDLE,
                        "--source", source, "--destination", str(destination))

    def stop_app(self, directory):
        record = Path(directory) / "processes.json"
        try:
            self.run("info", "processes", "--json-output", str(record), "--quiet")
            data = json.loads(record.read_text(encoding="utf-8"))["result"]
            for process in data.get("runningProcesses", []):
                executable = process.get("executable", "")
                if isinstance(executable, dict):
                    executable = executable.get("path", "")
                if str(executable).endswith("/NanoMusicPlayer.app/NanoMusicPlayer"):
                    self.run("process", "terminate", "--pid", str(process["processIdentifier"]))
        finally:
            record.unlink(missing_ok=True)

    def launch(self):
        self.run("process", "launch", "--terminate-existing", BUNDLE)


def read_manifest(package):
    manifest = json.loads((Path(package) / "manifest.json").read_text(encoding="utf-8"))
    if (not isinstance(manifest, dict) or manifest.get("version") != 1 or
            any(not isinstance(manifest.get(key), str) or re.fullmatch("[0-9a-f]{64}", manifest[key]) is None
                for key in ("source_id", "export_id")) or
            not isinstance(manifest.get("tracks"), list) or not isinstance(manifest.get("playlists"), list)):
        raise ValueError("Invalid desktop sync package")
    if (Path(package) / "READY").exists() or (Path(package) / "READY").is_symlink():
        raise ValueError("Remove the local READY marker before delivering a package; commit must happen after copying")
    return manifest


def deliver_package(device, package, backup, wait_seconds=300, progress=print, *, events=None, cancel=None):
    package, backup = Path(package).resolve(), Path(backup).resolve()
    manifest = read_manifest(package)
    backup.mkdir(parents=True, exist_ok=True)
    restarted = False
    touched_app = False
    original_cancel = device.cancel if isinstance(device, Device) else None
    if cancel is None:
        cancel = original_cancel
    if isinstance(device, Device):
        device.cancel = cancel
    try:
        check_cancel(cancel)
        emit(events, "backup", "Backing up the phone before copying")
        touched_app = True
        device.stop_app(backup)
        check_cancel(cancel)
        progress("Backing up the phone's existing music database and preferences")
        device.copy_from("Documents/.nano", backup / "phone-support")
        check_cancel(cancel)
        remote = "Documents/DesktopSync/" + manifest["source_id"]
        emit(events, "copy", "Copying music and metadata over USB; byte progress is unavailable")
        progress("Copying music and metadata over USB")
        # A previous interrupted delivery may have left a valid READY. Invalidate
        # it before overwriting any payload, including when resending one export.
        with tempfile.TemporaryDirectory(prefix="nano-sync-invalidate-") as temporary:
            invalid = Path(temporary)
            (invalid / "READY").write_text("INCOMPLETE", encoding="ascii")
            # An earlier failure for this exact export must not masquerade as
            # the result of the retry while the phone is still importing.
            write_json(invalid / "report.json", dict(export_id=manifest["export_id"],
                                                     source_id=manifest["source_id"], error=None))
            device.copy_to(invalid, remote)
        check_cancel(cancel)
        device.copy_to(package, remote)
        check_cancel(cancel)
        # Crossing this event is the commit boundary. Cancel-file / signal
        # requests after it cannot interrupt READY upload or the phone's merge.
        emit(events, "ready", "Committing the copied package; cancellation is no longer available", cancellable=False)
        if isinstance(device, Device):
            device.cancel = None
        with tempfile.TemporaryDirectory(prefix="nano-sync-commit-") as temporary:
            ready = Path(temporary) / "READY"
            ready.write_text(manifest["export_id"], encoding="ascii")
            device.copy_to(ready, remote + "/READY")
        device.launch()
        restarted = True
        emit(events, "importing", "Phone is merging music data; keep the app open", cancellable=False)
        progress("Phone is merging music data; waiting for its completion receipt")
        deadline = time.monotonic() + wait_seconds
        while time.monotonic() < deadline:
            receipt_file = backup / "receipt.json"
            receipt_file.unlink(missing_ok=True)
            try:
                device.copy_from(remote + "/receipt.json", receipt_file)
                receipt = json.loads(receipt_file.read_text(encoding="utf-8"))
                if (receipt.get("complete") is True and receipt.get("export_id") == manifest["export_id"] and
                        receipt.get("source_id", manifest["source_id"]) == manifest["source_id"]):
                    emit(events, "complete", "Sync complete; phone edits were preserved", cancellable=False, receipt=receipt)
                    progress("Sync complete: {} tracks, {} playlists; {} conflicts kept on phone".format(
                        receipt.get("tracks", 0), receipt.get("playlists", 0), receipt.get("conflicts", 0)))
                    return receipt
            except (RuntimeError, OSError, json.JSONDecodeError):
                pass
            try:
                report_file = backup / "report.json"
                report_file.unlink(missing_ok=True)
                device.copy_from(remote + "/report.json", report_file)
                report = json.loads(report_file.read_text(encoding="utf-8"))
                if report.get("export_id") == manifest["export_id"] and report.get("error"):
                    raise ValueError("Phone could not import the package; its local music and edits were preserved")
            except (RuntimeError, OSError, json.JSONDecodeError):
                pass
            time.sleep(min(2, max(0, deadline-time.monotonic())))
        raise TimeoutError("The phone has not returned a completion receipt. Keep the app open; package and backup are preserved.")
    finally:
        # Re-enabling cancellation here could interrupt the recovery launch.
        if isinstance(device, Device):
            device.cancel = None
        try:
            if touched_app and not restarted:
                try:
                    device.launch()
                except (OSError, RuntimeError, subprocess.TimeoutExpired):
                    raise SyncError("restart_required", "Unable to restart the phone app; open Nano Music Player on the iPhone") from None
        finally:
            if isinstance(device, Device):
                device.cancel = original_cancel


class Cancellation:
    def __init__(self, path=None):
        self.path = path
        self.requested = False

    def __call__(self):
        return self.requested or (self.path is not None and self.path.exists())

    def request(self, _signum, _frame):
        self.requested = True


def safe_error(error):
    if isinstance(error, SyncError):
        return error.code, str(error)
    if isinstance(error, (subprocess.TimeoutExpired, TimeoutError)):
        return "timeout", "The operation timed out; keep the phone app open and check the USB connection"
    if isinstance(error, sqlite3.Error):
        return "library_unavailable", "Unable to read the selected desktop music library"
    if isinstance(error, (ValueError, KeyError, TypeError)):
        return "invalid_input", "The library, sync selection or package is invalid; refresh the panel and retry"
    return "sync_failed", "Unable to complete sync; check file permissions, free space and the USB connection"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--list-devices", action="store_true", help="List connected, paired USB iPhones/iPads")
    modes.add_argument("--inspect-library", action="store_true", help="Read available music and playlists without exporting")
    modes.add_argument("--plan", action="store_true", help="Preview the selected sync scope without exporting or device access")
    parser.add_argument("--json-events", action="store_true", help="Write only one JSON event per stdout line")
    parser.add_argument("--config-dir", type=Path, default=Path.home() / "Library/Application Support/Nano.Nano-Music-Player")
    parser.add_argument("--output", type=Path, default=Path("target/iphone-sync") / time.strftime("%Y%m%d-%H%M%S"))
    parser.add_argument("--package", type=Path, help="Deliver a previously exported package")
    parser.add_argument("--device", help="Connected USB device ID from --list-devices")
    parser.add_argument("--apply", action="store_true", help="Back up the phone, deliver and import the package")
    parser.add_argument("--scope", choices=("all", "playlists"), default="all")
    parser.add_argument("--playlist-id", action="append", type=int, help="Selected desktop playlist ID; repeat for multiple playlists")
    parser.add_argument("--include-lyrics", action=argparse.BooleanOptionalAction, default=True,
                        help="Include lyrics, associations, romaji and translations (default enabled)")
    parser.add_argument("--include-statistics", action=argparse.BooleanOptionalAction, default=True,
                        help="Include ratings, playback counts and listening history (default enabled)")
    parser.add_argument("--cancel-file", type=Path, help="Creating this file requests cancellation before READY; do not kill the worker")
    parser.add_argument("--wait-seconds", type=int, default=300, help="Maximum wait for the phone completion receipt")
    # argparse failures must obey NDJSON too; keep standard --help text for humans.
    arguments = sys.argv[1:] if argv is None else list(argv)
    json_mode = "--json-events" in arguments

    def events(event):
        print(json.dumps(event, ensure_ascii=False, allow_nan=False), flush=True)

    sink = events if json_mode else None
    progress = (lambda _message: None) if json_mode else print
    cancellation = Cancellation()
    old_handlers = {}
    try:
        if json_mode:
            if "--help" in arguments or "-h" in arguments:
                emit(sink, "help", parser.format_help(), cancellable=False)
                return 0
            def argument_error(_message):
                raise SyncError("invalid_arguments", "Invalid sync arguments; refresh the panel and retry")
            parser.error = argument_error
        args = parser.parse_args(arguments)
        cancellation.path = args.cancel_file
        # Only CLI entrypoints install handlers, never the reusable functions.
        if argv is None:
            for signum in (signal.SIGINT, signal.SIGTERM):
                old_handlers[signum] = signal.signal(signum, cancellation.request)
        if args.apply and (not args.device or args.list_devices or args.inspect_library or args.plan):
            raise SyncError("invalid_arguments", "--apply requires --device and cannot be combined with a preview command")
        if args.wait_seconds <= 0:
            raise SyncError("invalid_arguments", "Receipt wait time must be positive")
        check_cancel(cancellation)
        if args.list_devices:
            devices = list_devices()
            if sink:
                emit(sink, "devices", "Connected USB devices", devices=devices, cancellable=False)
            else:
                print(json.dumps(devices, ensure_ascii=False))
            return 0
        options = dict(scope=args.scope, playlist_ids=args.playlist_id,
                       include_lyrics=args.include_lyrics, include_statistics=args.include_statistics)
        if args.inspect_library or args.plan:
            if args.package:
                raise SyncError("invalid_arguments", "Preview commands use the desktop library, not --package")
            result = inspect_library(args.config_dir, **options, cancel=cancellation)
            phase = "library" if args.inspect_library else "plan"
            if sink:
                emit(sink, phase, "Desktop library inspected; no files or devices were changed", cancellable=False, **{phase: result})
            else:
                print(json.dumps(result, ensure_ascii=False, indent=2))
            return 0
        if args.apply:
            devices = list_devices()
            if args.device not in {d["id"] for d in devices}:
                raise SyncError("device_unavailable", "Selected iPhone is not connected over USB; unlock, trust and reconnect it")
        package = args.package or args.output
        if args.package:
            if args.scope != "all" or args.playlist_id is not None or not args.include_lyrics or not args.include_statistics:
                raise SyncError("invalid_arguments", "A saved package already has a fixed selection; export a new package to change it")
            manifest = read_manifest(package)
        else:
            manifest = export_package(args.config_dir, package, progress=progress, **options, events=sink, cancel=cancellation)
        progress("Prepared {} tracks, {} playlists at {}".format(len(manifest["tracks"]), len(manifest["playlists"]), package))
        if args.apply:
            receipt = deliver_package(Device(args.device), package, package.parent / (package.name + "-phone-backup"),
                                      wait_seconds=args.wait_seconds, progress=progress, events=sink, cancel=cancellation)
            if not json_mode:
                print(json.dumps(receipt, ensure_ascii=False))
        else:
            emit(sink, "exported", "Export complete; phone has not been changed", cancellable=False,
                 package=str(Path(package).resolve()), source_id=manifest["source_id"], export_id=manifest["export_id"],
                 tracks=len(manifest["tracks"]), playlists=len(manifest["playlists"]))
            progress("Export only. Use --package PATH --device UDID --apply to sync over USB.")
        return 0
    except (OSError, ValueError, RuntimeError, TimeoutError, subprocess.TimeoutExpired,
            sqlite3.Error, KeyError, TypeError) as error:
        code, message = safe_error(error)
        phase = "cancelled" if isinstance(error, Cancelled) else "error"
        emit(sink, phase, message, cancellable=False, code=code)
        print("Sync failed: " + message, file=sys.stderr)
        return 130 if isinstance(error, Cancelled) else 1
    finally:
        for signum, handler in old_handlers.items():
            signal.signal(signum, handler)


if __name__ == "__main__":
    sys.exit(main())
