# Original Settings icon source

`assets/original/settings.png` is the original iPod nano (7th generation) Settings icon recovered from Apple's iPod software update, not a newly drawn or generated substitute. The asset is the 112 × 112 pixel RGBA image with firmware image ID `229442205`.

Source firmware: [Apple iPod software 1.1.2, build 39A10023](https://appldnld.apple.com/ipod/sbml/osx/bundles/031-59796-20160525-8E6A5D46-21FF-11E6-89D1-C5D3662719FC/iPod_1.1.2_39A10023.ipsw). The image was decoded from the firmware's `SilverImagesDB.LE.bin` image table. The entry uses BGRA pixel format `0x1888`; its file header reports 112 × 112 dimensions. The checked-in PNG is the decoded source image with transparency preserved, without redrawing or resizing.

The extracted firmware/database was kept outside the repository. The generated Home icons remain separate and unchanged; this source asset is only for the Settings icon.
