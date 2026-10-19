# sqx-unpack

Open-source extractor for **SQX** archives (Squeez / SpeedCommander) — runs on Linux, macOS and Windows,
no Wine needed. 7-Zip, unar and libarchive do not read this format.

## Install
```
pipx install sqx-unpack        # prebuilt binary wheels, no Rust needed
# or
cargo install --git https://github.com/theundefined/sqx-unpack
```

## Usage
```
sqx l archive.spq              # list
sqx t archive.spq              # test (length + CRC-32)
sqx x archive.spq [OUTDIR]     # extract
```

## Status
Compression method 6 (PPMd var.I with SQX range coder) is supported and verified on real archives.
Not supported yet: other methods, filters, solid mode, encryption, multi-volume archives.
If you have such archives (without private data), please open an issue.
Speed is currently ~3 MB/s.

Format notes: [docs/SQX-format.md](docs/SQX-format.md). Legal notes and development process: [LEGAL.md](LEGAL.md).
License: GPL-3.0-or-later.
