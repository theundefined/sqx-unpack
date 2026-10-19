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

### Method 6: stream format (verified: both sample entries pass the CRC check)
* Each entry's data is an independent stream of `csize` bytes. Bytes are grouped in little-endian 32-bit
  words and handed to the coder from the most significant byte of each word to the least significant one
  (zero padding past the end). No bytes precede the coder.
* Coder: carry-less Subbotina, `low = 0`, `range = 0xFFFFFFFF`, `code` = first word. Normalise while
  `(low ^ (low + range)) < 2^24`, or `range < 0x10000` (then `range = -low & 0xFFFF`), shifting in the next
  byte. Symbol step: `range /= total; count = (code - low) / range; low += start * range;
  range *= size;` then normalise. Binary contexts use `total = 2^14`.
* Parameters are coded by this same coder as equiprobable bits (`range >>= 1; q = (code - low) / range;
  low += q * range;` MSB first): 1 flag bit (0 = fresh model; 1 not supported/unknown, probably "solid
  continuation"), 8 bits maximum order, 10 bits model memory in MB. Samples: 0, 8, 32.
* The symbols follow immediately; exactly `usize` symbols are decoded, there is no end marker.
* Model: PPMd var.I (Ppmd8) with the "restart" method only (full re-initialisation when memory runs out).
  Differences from 7-Zip's Ppmd8 are only the start-up tables: binary-context probability divisors per
  frequency class `{2,3,4,5,6,7,9,12,16,21,27,34,42,51,61,72,84,97,111,126,142,159,177,196,216}` and SEE
  bases `p_r = {1,2,3,5,8,12,17,23,30,38,47,57,68,80,93,107,122,138,155,173,192,212,233,255}` with
  `Summ = (2*p_r + 5) << 3`.
* `src/ppmd/` is bit-exact with 7-Zip's Ppmd8 when `Params::ppmd8_tables` is set and BOT = 2^15
  (dev tool `examples/refcheck.rs`).

## Unknown
Other methods, filters, solid mode, encryption, multivolume, header CRC16 algorithm.
