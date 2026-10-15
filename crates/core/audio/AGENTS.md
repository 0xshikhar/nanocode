Own audio duration probing and token accounting here. Keep nanocode-media::audio compatible. This crate must build independently of image codecs on native and WASM. Keep the existing supported containers and estimation behavior.

MPEG duration estimation preserves the previous Symphonia 0.6 sampling policy: stop after 17 frames or more than 16 KiB, whichever comes first. Fragmented MP4 timing uses the initial segment index or media header, without scanning subsequent media fragments.
