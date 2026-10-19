"""Exercise the Xcode/Cargo boundary without an installed iOS SDK."""

import json
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
BUILD_SCRIPT = ROOT / "scripts" / "build-ios-cargo.sh"


class IOSBuildTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="nano ios build ")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.tools = self.directory / ".cargo" / "bin"
        self.tools.mkdir(parents=True)
        self.log = self.directory / "commands.jsonl"
        self.environment = os.environ.copy()
        for variable in ("NANO_IOS_CARGO_TARGET_DIR", "DEVELOPER_DIR", "CARGO_PROFILE_DEV_OPT_LEVEL"):
            self.environment.pop(variable, None)
        self.environment.update(
            HOME=str(self.directory),
            PATH=f"{self.tools}:/usr/bin:/bin:/usr/sbin:/sbin",
            ARCHS="arm64",
            PLATFORM_NAME="iphonesimulator",
            CONFIGURATION="Debug",
            TARGET_BUILD_DIR=str(self.directory / "build products"),
            EXECUTABLE_PATH="NanoMusicPlayer.app/NanoMusicPlayer",
            DERIVED_FILE_DIR=str(self.directory / "derived files"),
            MOCK_LOG=str(self.log),
            CODE_SIGNING_ALLOWED="NO",
        )
        self.tool(
            "cargo",
            """
import json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ['MOCK_LOG'], 'a') as log:
    log.write(json.dumps(['cargo', args, os.environ['SDKROOT'], os.environ.get('CARGO_PROFILE_DEV_OPT_LEVEL')]) + '\\n')
target = args[args.index('--target') + 1]
profile = 'release' if '--release' in args else 'debug'
binary = pathlib.Path(os.environ['CARGO_TARGET_DIR']) / target / profile / 'nano-music-player'
binary.parent.mkdir(parents=True, exist_ok=True)
binary.write_text(target + '\\n')
""",
        )
        self.tool(
            "xcrun",
            """
import json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ['MOCK_LOG'], 'a') as log:
    log.write(json.dumps(['xcrun', args]) + '\\n')
if args[0] == '--sdk':
    print('/mock SDKs/' + args[1] + '.sdk')
elif args[0] == 'lipo':
    destination = pathlib.Path(args[args.index('-output') + 1])
    sources = args[args.index('-output') + 2:]
    destination.write_text(''.join(pathlib.Path(source).read_text() for source in sources))
elif args[0] == 'dsymutil':
    pathlib.Path(args[args.index('-o') + 1]).mkdir(parents=True)
else:
    raise SystemExit('unexpected mock xcrun arguments: ' + str(args))
""",
        )

    def tool(self, name, source):
        path = self.tools / name
        path.write_text(f"#!{sys.executable}\n" + source)
        path.chmod(0o755)

    def build(self, **variables):
        return subprocess.run(
            ["/bin/bash", str(BUILD_SCRIPT)],
            env={**self.environment, **variables},
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )

    def commands(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def binary(self):
        return (
            Path(self.environment["TARGET_BUILD_DIR"])
            / self.environment["EXECUTABLE_PATH"]
        )

    def test_debug_simulator_build_preserves_paths_and_locked_dependencies(self):
        result = self.build()
        self.assertEqual(result.returncode, 0, result.stderr)
        cargo = next(command for command in self.commands() if command[0] == "cargo")
        self.assertIn("aarch64-apple-ios-sim", cargo[1])
        self.assertIn("--locked", cargo[1])
        self.assertEqual(cargo[2], "/mock SDKs/iphonesimulator.sdk")
        self.assertEqual(cargo[3], "2", "device/preview UI must not accidentally ship unoptimized Rust dependencies")
        self.assertEqual(self.binary().read_text(), "aarch64-apple-ios-sim\n")
        self.assertEqual(
            (self.binary().parent / "licenses" / "NotoSansCJK-OFL.txt").read_bytes(),
            (ROOT / "ui" / "fonts" / "noto-sans-cjk" / "LICENSE").read_bytes(),
        )

    def test_simulator_universal_build_selects_both_architectures(self):
        result = self.build(ARCHS="arm64 x86_64")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            self.binary().read_text(),
            "aarch64-apple-ios-sim\nx86_64-apple-ios\n",
        )

    def test_device_release_build_and_matching_symbols(self):
        symbols = self.directory / "debug symbols"
        result = self.build(
            PLATFORM_NAME="iphoneos",
            CONFIGURATION="Release",
            DWARF_DSYM_FOLDER_PATH=str(symbols),
            DWARF_DSYM_FILE_NAME="NanoMusicPlayer.app.dSYM",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        cargo = next(command for command in self.commands() if command[0] == "cargo")
        self.assertIn("--release", cargo[1])
        self.assertEqual(self.binary().read_text(), "aarch64-apple-ios\n")
        self.assertTrue((symbols / "NanoMusicPlayer.app.dSYM").is_dir())

    def test_unsupported_device_architecture_fails_without_cargo(self):
        result = self.build(PLATFORM_NAME="iphoneos", ARCHS="x86_64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Unsupported iOS platform/architecture", result.stderr)
        self.assertFalse(any(command[0] == "cargo" for command in self.commands()))

    def test_missing_xcode_settings_reports_how_to_run_the_script(self):
        result = self.build(ARCHS="")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing Xcode build setting ARCHS", result.stderr)

    def test_app_declares_scene_lifecycle_audio_and_usb_file_sharing(self):
        with (ROOT / "ios" / "Info.plist").open("rb") as source:
            info = plistlib.load(source)
        self.assertIn("audio", info["UIBackgroundModes"])
        self.assertTrue(info["UIFileSharingEnabled"])
        scenes = info["UIApplicationSceneManifest"]["UISceneConfigurations"]
        self.assertEqual(
            scenes["UIWindowSceneSessionRoleApplication"][0]["UISceneDelegateClassName"],
            "SlintWindowSceneDelegate",
        )


if __name__ == "__main__":
    unittest.main()
