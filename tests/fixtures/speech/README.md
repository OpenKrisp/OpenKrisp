# Speech test fixtures

Two untrimmed clips from [LibriSpeech dev-clean](https://www.openslr.org/12), derived from LibriVox recordings and distributed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).

Attribution: Vassil Panayotov, Guoguo Chen, Daniel Povey and Sanjeev Khudanpur, *LibriSpeech: An ASR corpus based on public domain audio books* (2015).

`manifest.json` records the official archive URL, original paths and checksums. The checked-in files contain mono 16 kHz signed 16-bit little-endian PCM, losslessly decoded from FLAC with SoundFile 0.13.1. No trimming or level changes were applied to the stored assets. `tests/support/speech.rs` applies the documented deterministic rate/level conversion and generated noise mixtures during tests. These fixtures are test data and are not bundled with the desktop application.
