# Synthetic audio fixtures

`tone.mp3` and `tone.ogg` are generated 0.25-second, 440 Hz mono sine waves, not client assets. They exercise the Windows rodio MP3 and Vorbis decoders in `audio-corpus-check` tests. The WAV fixtures are generated in the test itself.

Generated with FFmpeg 9.0.2 from `sine=frequency=440:sample_rate=22050:duration=0.25`, using libmp3lame at 48 kbit/s (no ID3 or Xing headers) and libvorbis at quality 1. No game audio is included.
