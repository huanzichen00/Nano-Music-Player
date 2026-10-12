# Reference material and reused resources

## Device behavior and original artwork

- [Apple iPod nano manuals](https://support.apple.com/en-us/docs/ipod/133772).
- The supplied Apple 2012 iPod nano (7th generation) user guide (`46034.pdf`): rendered pages 9–10 and 18–20 and inspected music/podcast chapters. The page-10 Flick example contains the original Albums list screenshot used to measure row, thumbnail and header proportions. Playback layout and alphabet navigation were checked against this original manual.
- [Apple seventh-generation technical specifications](https://support.apple.com/en-us/112039): 240×432 LCD and 39.6×76.5 mm complete hardware dimensions used for the shared display/device proportions.
- [Apple iPod nano User Guide, 2015](https://cdsassets.apple.com/live/6GJYWVAV/user/locale/en-gb/ma1624_ipod_nano_2015_user_guide.pdf): Home icon arrangement, Home-button behavior, touch scrolling, Music browsing and Now Playing controls. The six Home icons under `assets/original/` contain the guide's original 53×53 images and their transparency masks extracted from page 5. The SVG wrappers restore the PDF masks; they do not redraw the symbols. The low resolution limits clarity on Retina displays. Apple artwork remains © Apple Inc. and has no open-source license granted here.
- [AnandTech: iPod Nano 7th Generation Brief Hands On](https://www.youtube.com/watch?v=sGgejvq2vlE): found as an early device reference. The in-app browser cannot play its media, and the brief video lacks useful music interaction coverage. Do not treat it as verification of the music UI.
- [Justin Tse: iPod Nano 7th Generation Review](https://www.youtube.com/watch?v=SEvDPVUbZns): additional video candidate; playback details have not been visually verified. Further music-focused reference footage is pending.

## Implementation resources

- [Slint 1.18.1](https://docs.rs/slint/1.18.1/slint/): declarative UI, a single root capture handler, native modifier normalization, menu bar, scrolling and window dragging.
- [Slint fancy_demo slider](https://github.com/slint-ui/slint/blob/v1.18.1/examples/fancy_demo/main.slint): drag mapping adapted in `ui/components.slint`, under the upstream MIT examples license preserved in `licenses/slint-examples-MIT.txt`.
- [Font Awesome Free 7](https://github.com/FortAwesome/Font-Awesome/tree/7.x/svgs/solid): SVG symbols for playback, search and fallback imagery. Upstream notices remain in each SVG; full license is in `licenses/font-awesome.txt`. Icons are CC BY 4.0.
- [Rodio 0.21.1](https://docs.rs/rodio/0.21.1/rodio/) and [Symphonia](https://github.com/pdeljanov/Symphonia): audio output, position, seeking and codec decoding.
- [Lofty 0.22.4](https://docs.rs/lofty/0.22.4/lofty/): metadata and MP3 synchronized lyric parsing.
- [lrc 0.2.0](https://docs.rs/lrc/0.2.0/lrc/): LRC metadata and timestamp collection. The app normalizes decimal timestamps to cover tenths/hundredths/milliseconds correctly.
- [slix-player](https://github.com/cetra3/slix-player): architectural reference for Slint with Rust audio. No AGPL source was copied.

All audio fixtures are original quiet synthetic tones generated locally; encoder commands and the Lofty test-writing workaround are documented in `tests/fixtures/README.md`. Application import never modifies audio tags.

## Generated icon variants

Sharper, transparent icon reconstructions are generated with the built-in imagegen tool from the original guide icons. They are derived approximations, not newly recovered original Apple assets. Outputs, prompts and chosen assets are saved in `assets/generated/`; the original extracted assets remain available for comparison.

## Current player references

- [Apollomaniacs: seventh-generation nano Music](https://www.apollomaniacs.com/ipod/howto_music_ipodnano.htm): supplied by the user; cover tap reveals extra controls and lyrics over artwork, and the track-list button opens the current album.
- The four user-supplied photographs guide the charcoal title strip, full-width artwork, large white transport symbols, compact lyrics and blue aluminium device frame. The fourth photograph is the primary player reference.
- The original 112×112 Settings icon was recovered directly from Apple's firmware; see [source and extraction details](SETTINGS_ICON_SOURCE.md). The user's generated Home icons are retained.
- [Yazi Quick Start](https://yazi-rs.github.io/docs/quick-start/): H leaves the current level, L enters, J/K move the cursor. Letters keep those meanings in lists, while Home uses H/L for horizontal icon navigation; seek and volume controls use separate actions or arrow context.
- `previous.svg`, `next.svg`, `track-list.svg` and `slider-knob.svg` are small vector assets authored for this project. The unused legacy `lyrics.svg` remains in the asset directory; the redundant lyrics button has been removed.
