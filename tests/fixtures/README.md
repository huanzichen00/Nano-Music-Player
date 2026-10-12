These four fixtures contain the same original synthetic 440 Hz tone: 0.25 seconds,
mono, 44.1 kHz, approximately -50 dBFS, with fades at both ends. No music or
third-party recordings are included. They have no title, artist, or lyrics tags.

`generate.py` creates 16-bit PCM WAV using Python's standard library, then encodes
MP3 with LAME, FLAC with the FLAC CLI, and AAC in an M4A container with macOS
`afconvert`. Run `python3 tests/fixtures/generate.py` to regenerate them on a Mac
with `lame` and `flac` installed. The regression tests use the committed files
and do not require those encoders at runtime.

Tests write metadata and lyrics to temporary copies with Lofty. The original
fixtures remain unchanged. Decoder checks consume sample data without opening
an audio device or making sound.

The FLAC includes 64 bytes of metadata padding. Lofty 0.22.4 can corrupt an
unpadded FLAC while adding its first padding block, so an existing small block
lets these tests add lyrics without exercising that unrelated writer bug.
