# Noto Sans CJK SC

`NotoSansCJKsc-Regular.otf` is the unmodified Simplified Chinese face from
[Noto CJK](https://github.com/notofonts/noto-cjk/tree/main/Sans/OTF/SimplifiedChinese),
version 2.004, distributed under the SIL Open Font License 1.1 in `LICENSE`.

SHA-256: `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`.

Only `ui/mobile.slint` imports the font. Slint embeds it into the mobile binary;
the ordinary desktop build continues using its existing system fonts. The full
font is intentional: UI-only subsetting would leave arbitrary imported song
names, artist names and lyrics without glyphs. Its family is `Noto Sans CJK SC`.

`scripts/build-ios-cargo.sh` copies `LICENSE` into the iOS app bundle as
`licenses/NotoSansCJK-OFL.txt`.
