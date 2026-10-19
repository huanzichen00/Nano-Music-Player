#!/usr/bin/env bash
# Supplementary Rust-only bridge check. This is not an iOS SDK build.
# Uses existing host .rlib dependencies from a prior cargo build/test.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 - <<'PY'
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path.cwd()
cargo = os.environ.get("CARGO") or shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
rustc = os.environ.get("RUSTC") or shutil.which("rustc") or str(Path.home() / ".cargo/bin/rustc")
metadata = json.loads(subprocess.check_output([cargo, "metadata", "--offline", "--no-deps", "--format-version", "1"], text=True))
dependencies = Path(metadata["target_directory"]) / "debug/deps"
flags = [rustc, "--edition=2024", "-A", "dead_code", "-A", "unused_imports", "-L", "dependency=" + str(dependencies)]
for name in ["serde_json", "rfd", "directories", "slint", "tempfile"]:
    candidates = list(dependencies.glob("lib" + name + "-*.rlib"))
    if not candidates:
        raise SystemExit("Missing host " + name + " artifact. Run cargo test --features mobile --lib --no-run first.")
    artifact = max(candidates, key=lambda path: path.stat().st_mtime)
    flags += ["--extern", name + "=" + str(artifact)]

# Use the actual files and shared types. The separate harness avoids injecting
# an artificial target_os into the application or any dependency crate.
source = "mod platform {\n"
for name in ["macos_media", "ios"]:
    path = str(root / "src/platform" / (name + ".rs"))
    source += "#[path = " + json.dumps(path) + "] pub mod " + name + ";\n"
source += "}\n"
source += "mod shared { #[path = " + json.dumps(str(root / "src/shared/diagnostics.rs")) + "] pub mod diagnostics; }\n"
os.environ["CARGO_PKG_VERSION"] = metadata["packages"][0]["version"]
with tempfile.TemporaryDirectory(prefix="nano-ios-bridge-check-") as directory:
    temporary = Path(directory)
    wrapper = temporary / "bridge.rs"
    wrapper.write_text(source)
    binary = temporary / "bridge-tests"
    subprocess.run(flags + ["--test", str(wrapper), "-o", str(binary)], check=True)
    # Filter out macOS native integration tests: this helper never registers
    # remote controls, presents a picker, or activates an audio session.
    subprocess.run([str(binary), "platform::ios::tests"], check=True)
    subprocess.run(flags + ["--crate-type=lib", "--emit=metadata", "--cfg", 'target_os="ios"',
        "-A", "explicit_builtin_cfgs_in_flags", str(wrapper), "-o", str(temporary / "ios-bridge.rmeta")], check=True)
print("Bridge parser tests and isolated Rust iOS branch type-check passed.")
print("This does not compile NativeBridge.m, link an iOS binary, or verify device behavior.")
PY
