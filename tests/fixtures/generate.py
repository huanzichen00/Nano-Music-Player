#!/usr/bin/env python3
"""Recreate the original, quiet audio fixtures without downloading media."""

import math
from pathlib import Path
import struct
import subprocess
import wave


fixture_dir = Path(__file__).resolve().parent
wav_path = fixture_dir / "quiet.wav"
rate = 44_100
frames = rate // 4
samples = []
for index in range(frames):
    # A 440 Hz tone at approximately -50 dBFS, with short fades at both ends.
    envelope = min(1.0, index / 441, (frames - 1 - index) / 441)
    samples.append(round(100 * envelope * math.sin(2 * math.pi * 440 * index / rate)))

with wave.open(str(wav_path), "wb") as output:
    output.setparams((1, 2, rate, frames, "NONE", "not compressed"))
    output.writeframes(struct.pack(f"<{frames}h", *samples))

commands = [
    ["lame", "--silent", "--noreplaygain", "-b", "64", "-m", "m", str(wav_path), str(fixture_dir / "quiet.mp3")],
    ["flac", "--silent", "--force", "--padding=64", "--no-seektable", "-o", str(fixture_dir / "quiet.flac"), str(wav_path)],
    ["afconvert", "-f", "m4af", "-d", "aac", "-b", "64000", str(wav_path), str(fixture_dir / "quiet.m4a")],
]
for command in commands:
    subprocess.run(command, check=True)

for extension in ("wav", "mp3", "flac", "m4a"):
    path = fixture_dir / f"quiet.{extension}"
    print(f"{path.name}: {path.stat().st_size} bytes")
