# Synthetic SQX test archive

`synthetic.sqx` is a small SQX archive (method 6 entries only) built from
**purely synthetic, deterministically generated content**; it contains no data
from any third-party archive. It is used by `tests/e2e.rs`.

| entry | content |
|---|---|
| `hello.txt` | short ASCII text |
| `repeat.txt` | repetitive text, lines built by a fixed arithmetic rule |
| `random.bin` | 24000 xorshift64 pseudo-random bytes (fixed seed) |
| `restart.dat` | 160000 pseudo-random bytes over a 4-letter alphabet, coded with order 16 and the minimum 1 MB model memory, so the PPMd model is exhausted and restarted (`mkarchive` verifies this and prints it) |
| `empty.txt` | empty file |

`synthetic.sha256` lists the SHA-256 of every entry in `sha256sum` format.
Header CRC16 fields = low 16 bits of the raw CRC-32 (init 0xFFFFFFFF, no final xor) of the header from its type byte on;
the per-entry CRC field is `~crc32(data)`.

## Regenerating

The archive is produced by the test-only PPMd encoder in
`src/ppmd/model/encoder.rs` (behind the `encoder` cargo feature) through
`examples/mkarchive.rs`:

    cargo run --release --features encoder --example mkarchive -- tests/data

The output is byte-for-byte reproducible (no timestamps, fixed seeds).
To check it independently of the Rust test:

    cargo run --release -- x tests/data/synthetic.sqx /tmp/out
    (cd /tmp/out && sha256sum -c /path/to/tests/data/synthetic.sha256)

(`cargo test --release` runs the same comparison automatically.)
