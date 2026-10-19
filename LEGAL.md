# Legal notes and development process

**This is a good-faith summary written by the authors, not legal advice.** If you
redistribute or build on this project commercially, get your own opinion.

## Purpose
`sqx-unpack` lets people read **their own** SQX archives (created by Squeez /
SpeedCommander, SpeedProject) on systems where the original program is not
available. There is no other open-source tool that reads this format.

## What this repository contains
- Original Rust code and original documentation of the file format.
- It does **not** contain any code, binaries, tables or resources of SpeedCommander /
  Squeez or any other third-party program, and no archives or data belonging to anyone.
- The compression method is PPMd variant I by Dmitry Shkarin; the reference
  implementation (`Ppmd8.c` from 7-Zip) is public domain. The model here was implemented
  independently and checked for bit-exact agreement with it on synthetic data.

## Why we believe this is permissible
- Copyright protects expression (code), not file formats, algorithms or ideas
  (Directive 2009/24/EC recital 11 and art. 1(2); in Poland: Prawo autorskie art. 74 ust. 4 / art. 1 ust. 2^1).
- Observing, studying and testing the behaviour of a program to learn the ideas and
  principles behind it is allowed (art. 5(3) of the Directive; art. 75 ust. 2 pkt 2 PrAut).
- Reproducing/translating code is allowed where indispensable to obtain the information
  necessary for interoperability of an independently created program (art. 6 of the Directive;
  art. 75 ust. 2 pkt 3 PrAut), provided it is not used to create a substantially similar program.
  The result of this analysis is the *format description* in `docs/SQX-format.md`;
  no vendor code is reused.
- Other jurisdictions (e.g. US DMCA 1201(f), interoperability exemption) have comparable but
  not identical rules. "SQX", "Squeez" and "SpeedCommander" are trademarks of their owners;
  this project is not affiliated with or endorsed by them.

## Development process (clean room)
Two separated roles:
1. **Analysis (role A).** A reverse-engineering prototype (a Python port of the decoder
   found in a publicly downloadable SpeedCommander installer, studied via static analysis
   and emulation in a sandbox) was used to learn how SQX differs from the public PPMd
   var.I algorithm. Role A produced a *prose description* of the differences (stream framing,
   parameter encoding, two initial tables, restart policy). The prototype, the emulator
   and the vendor installer are **kept private and are not part of this repository or its history**.
2. **Implementation (role B).** The decoder in `src/` was written by a separate agent
   that was forbidden to read the prototype, the emulator, the vendor binary or the
   handoff notes. It worked only from the public PPMd var.I source, a generic description of the
   range coder and, in the last step, role A's prose description of the differences.
   It was validated bit-for-bit against the public Ppmd8 implementation and by CRC-32
   of the extracted files.

Honest limits of this process: both roles were performed by AI agents under the same
maintainer, and role A's prose description (a few formulas and small constants) derives from
the prototype. The two initial tables are the only vendor-specific numeric data in the code;
they are functional facts required for interoperability.

## Other implementations
After this project was started we found other public work on the format, notably
[Squeez-c](https://github.com/seb3773/Squeez-c) (MIT). `sqx-unpack` was developed independently
of it; we did not read its source. A third-party repository that hosts the original
vendor SDK also exists; we did **not** use it. Squeez-c's test archives (not included in this repo) were used
only as an additional correctness check of our decoder.

## Test data
Only synthetic data is committed. Private or production archives are never committed.
If you believe something in this repository should not be here, open an issue
and it will be reviewed promptly.

## Warranty
GPL-3.0-or-later, no warranty. Always verify extracted files (the tool checks length and CRC-32).
