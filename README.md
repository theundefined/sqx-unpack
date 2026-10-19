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

## Related projects
- [Squeez-c](https://github.com/seb3773/Squeez-c) (MIT) is a broader C reimplementation of Squeez
  (methods 0-6, SQX 1.1 and 2.0, SFX, solid, and a compressor). If you need other methods or want to
  create archives, use it. `sqx-unpack` is a small, separately developed method-6 decoder that is
  easy to install (`pipx`); the format knowledge was cross-checked against it, not copied from it.
- Both tools install a binary named `sqx`; install only one of them in the same `PATH`.

## Status
Compression method 6 (PPMd var.I with SQX range coder) is supported and verified on real archives.
Not supported yet: other methods, filters, solid mode, encryption, multi-volume archives.
If you have such archives (without private data), please open an issue.
Speed is currently ~3 MB/s.

Verified on real archives, including all method-6 entries of the Squeez-c test corpus (made with the
official `sqc`). Entries using other methods are reported as unsupported.

Format notes: [docs/SQX-format.md](docs/SQX-format.md). Legal notes and development process: [LEGAL.md](LEGAL.md).
License: GPL-3.0-or-later.
