"""Check the bundled font's actual Unicode map, without host system fonts."""

from pathlib import Path
from struct import unpack_from
import unittest


ROOT = Path(__file__).resolve().parents[2]


class MobileFontTests(unittest.TestCase):
    def test_embedded_font_covers_interface_and_cjk_metadata(self):
        data = (ROOT / "ui/fonts/noto-sans-cjk/NotoSansCJKsc-Regular.otf").read_bytes()
        self.assertEqual(data[:4], b"OTTO")
        tables = {}
        for i in range(unpack_from(">H", data, 4)[0]):
            tag, _, offset, length = unpack_from(">4sIII", data, 12 + 16 * i)
            tables[tag] = data[offset:offset + length]

        cmap = tables[b"cmap"]
        covered = set()
        for i in range(unpack_from(">H", cmap, 2)[0]):
            platform, _, offset = unpack_from(">HHI", cmap, 4 + 8 * i)
            if platform not in (0, 3) or unpack_from(">H", cmap, offset)[0] != 12:
                continue
            for j in range(unpack_from(">I", cmap, offset + 12)[0]):
                start, end, glyph = unpack_from(">III", cmap, offset + 16 + 12 * j)
                # Glyph zero is .notdef, which produces the missing-glyph box.
                covered.update(range(start if glyph else start + 1, end + 1))
            break
        self.assertTrue(covered, "Font must have a full Unicode cmap")

        text = (ROOT / "src/app/i18n.rs").read_text()
        text += "".join(p.read_text() for p in (ROOT / "ui").rglob("*.slint"))
        text += "晴天 周杰倫 久石譲 さくら 한글 한국 음악"
        required = {
            ord(c) for c in text
            if 0x3000 <= ord(c) <= 0x9FFF
            or 0xAC00 <= ord(c) <= 0xD7AF
            or 0xFF00 <= ord(c) <= 0xFFEF
        }
        missing = required - covered
        self.assertFalse(missing, "Missing glyphs: " + "".join(map(chr, sorted(missing))))


if __name__ == "__main__":
    unittest.main()
