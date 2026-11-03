# Audio audit — 2026-11-03

## Scope and evidence

The supplied `Games/pw142` client was inspected. Its `element/userdata/SystemSettings.ini` contains separate `SoundVol` and `MusicVol` values. These are evidence of two controls, not proof of the client's gain curves or defaults. Explorer persists its own settings; it does not read or write the game's configuration.

The actual `sfx.pck` was reconstructed locally from the three supplied ZIP parts. The v2 package directory contains 1,999 entries: 1,907 WAV, 48 OGG, one MP3, 28 PK, five SFK, two TXT and eight extensionless entries. All 1,956 files with audio extensions were read and fully decoded without reported errors by an **independent FFmpeg 9.0.2 decoder**, using `tools/audio-census.py --ffmpeg ...`. This is not a test of Explorer's rodio decoder. WAV headers comprise 1,894 mono 16-bit PCM at 22,050/44,100/48,000 Hz, 12 stereo 16-bit PCM at 22,050 Hz, and one mono 32-bit IEEE-float file at 22,050 Hz. OGG is a container; the previous investigation verified a client track as Vorbis, but this audit did not separately verify the codec in every OGG. Metadata PK/SFK entries were not treated as playable audio.

Drive metadata also shows 36 loose MP3 files in `element/music` and 75 loose WAV files in `element/music/ambiencestereo`, plus a zero-length extensionless marker in the latter. Three loose samples (two WAV and one MP3) were independently decoded with FFmpeg, not the full loose set. No assumption is made that these counts cover every folder in the client. Assets are not included in the repository.

The supplied UTF-16 `maps/world/precinct.clt` parses as 332 zones, with ambience filenames before and after each music playlist; filenames strongly suggest day/night use, but native selection logic and transitions are not established. Explorer now retains both fields, offers independently enabled experimental stereo zone ambience and an independent sound volume slider, and uses the viewer's existing daylight threshold and smallest-polygon heuristic. It stops/restarts on selection changes rather than crossfading. The existing music slider controls only music. Both are off by default. These approximations are **not** full native audio parity.

Music archive lookup now tries the requested extension first and only then substitutes OGG for missing MP3. Tests cover that preference and path traversal rejection. Zone ambience plays only loose `music/ambiencestereo` assets; packed spatial ambience, world sound sources, creature/combat, interface, weather, and other game audio are not implemented. The archive census does not establish that Explorer can select or play those files.

## Verification and remaining gates

- Standalone Rust tests passed for settings persistence/bounds and music/ambience parser, lookup, day/night selection and disable behavior. The music tests used a stub PCK module; a separate test parsed the supplied real precinct file. They do not exercise the Windows backend.
- Full `cargo check` could not run: this sandbox cannot reach `index.crates.io` and had no cached dependencies. The new Windows-only `audio-corpus-check` can exercise rodio against `sfx.pck` and the loose music tree, but has not been run here; its iterator can still stop silently on some mid-stream errors. Windows compilation, rodio decoding of every file (including float WAV), real audio device playback, output-device recovery, gain laws, timing and crossfades remain unverified.
- To claim full correctness, test all loose and packed audio using the actual Windows decoder and native hardware, resolve client source/region/priority and timing behavior, implement positional/gameplay events, and compare against the real client. Do not interpret the FFmpeg census as completion of those gates.
