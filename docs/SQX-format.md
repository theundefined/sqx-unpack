# SQX container format (Squeez / SpeedCommander) — working notes

Status: partial. Only what has been observed and verified on real archives is listed.
All integers are little-endian.

## Main header
Starts with bytes `50 xx xx 52`; the string `-sqx-` is at offset 7. `u16` at offset 5 is the
main header size. Following blocks start right after it.

## Blocks
Each block starts with `u16 crc16, u8 type, u16 flags, u16 header_size`.

- If `flags & 0x8000`, extension headers follow the block, each of the same
  `u16 crc16, u8 type, u16 flags, u16 size` form; skip them (repeat while bit 0x8000 is set).
- type `'D'`: file entry (below). Other types (`'S'`, `'o'`, ...) are skipped by `header_size`.
  type `'X'` or `header_size == 0` ends the archive.

### File entry `'D'` (offsets from block start)
| offset | size | meaning |
|---|---|---|
| 7 | 1 | unknown (0 in samples) |
| 8,9,10 | 1 each | unknown |
| 11 | 1 | type (0x14 in samples) |
| 12 | 1 | compression method (6 = PPMd variant I with custom range coder) |
| 13 | 4 | `~crc32(uncompressed data)` i.e. `crc32 ^ 0xFFFFFFFF` |
| 0x11 | 4 | attributes |
| 0x15 | 4 | mtime (MS-DOS format) |
| 0x19 | 4/8 | compressed size (`u64` if `flags & 0x80`) |
| 0x1d / 0x21 | 4/8 | uncompressed size |
| 0x21 / 0x29 | 2 | name length |
| 0x23 / 0x2b | n | name (single-byte code page) |

With `flags & 0x80` sizes are 64-bit and the layout shifts as shown (second offset).
Entry data follows the block and its extension headers; it is `compressed size` bytes long.

## Method 6
PPMd variant I (Shkarin, as in 7-Zip `Ppmd8`) with a different range coder:
carry-less (Subbotin style), bottom limit `0x10000`; the bit stream is read as
little-endian 32-bit words. Observed parameters: model order 8, 32 MB model memory,
model restart when memory is exhausted. See TODO list in README for unknowns.

## Unknown
Other methods, filters, solid mode, encryption, multivolume, header CRC16 algorithm.
