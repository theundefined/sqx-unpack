//! Builds the synthetic SQX test archive `tests/data/synthetic.sqx` (plus
//! `synthetic.sha256`) from purely synthetic, deterministically generated
//! content.  Not a general-purpose compressor.
//!
//!   cargo run --release --features encoder --example mkarchive -- tests/data
use sqx_unpack::crc32::Crc32;
use sqx_unpack::ppmd::encode_sqx;

/// xorshift64 with a fixed seed: platform independent pseudo-random bytes.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn short_text() -> Vec<u8> {
    b"Hello, SQX!\nThis is a tiny synthetic text written for the sqx-unpack test suite.\n".to_vec()
}

/// Repetitive text: line i is built from fixed word lists by simple arithmetic.
fn repeat_text() -> Vec<u8> {
    const W: [&str; 8] = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel"];
    let mut s = String::new();
    for i in 0..600u32 {
        s.push_str(&format!(
            "line {:04}: {} {} {} value={}\n",
            i,
            W[(i % 8) as usize],
            W[((i * 3 + 1) % 8) as usize],
            W[((i / 5) % 8) as usize],
            (i * i) % 97
        ));
    }
    s.into_bytes()
}

fn random_bytes(n: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng(seed);
    (0..n).map(|_| (r.next() >> 24) as u8).collect()
}

/// Low-entropy random data over a 4-letter alphabet; with order 16 and the
/// minimum 1 MB of model memory this exhausts the model several times.
fn restart_data(n: usize) -> Vec<u8> {
    let mut r = Rng(0x5eed_0002);
    (0..n).map(|_| b"acgt"[((r.next() >> 33) & 3) as usize]).collect()
}

struct Item {
    name: &'static str,
    data: Vec<u8>,
    order: u32,
    mem_mb: u32,
}

fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut m = data.to_vec();
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for blk in m.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(blk[4 * i..4 * i + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// Header CRC16 (bytes 0..2): low 16 bits of the raw CRC-32 (init 0xFFFFFFFF,
/// no final inversion) of the header bytes from the type byte to its end.
fn set_header_crc(h: &mut [u8]) {
    let mut c = Crc32::new();
    c.update(&h[2..]);
    h[0..2].copy_from_slice(&(!c.finish() as u16).to_le_bytes());
}

fn entry_block(name: &str, data: &[u8], stream: &[u8]) -> Vec<u8> {
    let mut c = Crc32::new();
    c.update(data);
    let crc = !c.finish();
    let hsz = 0x23 + name.len();
    let mut h = vec![0u8; hsz];
    h[2] = b'D';
    h[3..5].copy_from_slice(&0x0014u16.to_le_bytes()); // flags: 32-bit sizes
    h[5..7].copy_from_slice(&(hsz as u16).to_le_bytes());
    h[11] = 0x14;
    h[12] = 6; // method 6
    h[13..17].copy_from_slice(&crc.to_le_bytes());
    h[0x11..0x15].copy_from_slice(&0x20u32.to_le_bytes()); // attributes: archive
    h[0x15..0x19].copy_from_slice(&0x5a21_6000u32.to_le_bytes()); // fixed DOS mtime
    h[0x19..0x1d].copy_from_slice(&(stream.len() as u32).to_le_bytes());
    h[0x1d..0x21].copy_from_slice(&(data.len() as u32).to_le_bytes());
    h[0x21..0x23].copy_from_slice(&(name.len() as u16).to_le_bytes());
    h[0x23..].copy_from_slice(name.as_bytes());
    set_header_crc(&mut h);
    h.extend_from_slice(stream);
    h
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "tests/data".into());
    let items = vec![
        Item { name: "hello.txt", data: short_text(), order: 8, mem_mb: 32 },
        Item { name: "repeat.txt", data: repeat_text(), order: 8, mem_mb: 32 },
        Item { name: "random.bin", data: random_bytes(24_000, 0x5eed_0001), order: 8, mem_mb: 32 },
        Item { name: "restart.dat", data: restart_data(160_000), order: 16, mem_mb: 1 },
        Item { name: "empty.txt", data: Vec::new(), order: 8, mem_mb: 32 },
    ];
    let mut arc = vec![0u8; 25];
    arc[2] = b'R'; // main header: u16 crc16 (not verified here), type 'R', u16 flags, u16 size
    arc[5..7].copy_from_slice(&25u16.to_le_bytes());
    arc[7..12].copy_from_slice(b"-sqx-");
    arc[12] = 0x0b;
    set_header_crc(&mut arc);
    let mut sums = String::new();
    for it in &items {
        let stream = encode_sqx(&it.data, it.order, it.mem_mb);
        // A restart happened iff a 64 MB model (no exhaustion) yields a different size.
        let restarted = it.mem_mb < 64 && encode_sqx(&it.data, it.order, 64).len() != stream.len();
        eprintln!("{:12} {:>7} -> {:>7} bytes (order {}, {} MB, model restarted: {})", it.name, it.data.len(), stream.len(), it.order, it.mem_mb, restarted);
        arc.extend(entry_block(it.name, &it.data, &stream));
        sums.push_str(&format!("{}  {}\n", sha256(&it.data), it.name));
    }
    let mut end = [0, 0, b'X', 0, 0, 7, 0]; // end-of-archive block
    set_header_crc(&mut end);
    arc.extend_from_slice(&end);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(format!("{dir}/synthetic.sqx"), &arc).unwrap();
    std::fs::write(format!("{dir}/synthetic.sha256"), sums).unwrap();
    eprintln!("wrote {dir}/synthetic.sqx ({} bytes)", arc.len());
}
