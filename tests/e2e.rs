//! End-to-end test: the `sqx` binary must test and extract the synthetic
//! archive `tests/data/synthetic.sqx`, and every extracted file must match the
//! SHA-256 listed in `tests/data/synthetic.sha256` (see tests/data/README.md).

use std::path::{Path, PathBuf};
use std::process::Command;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data")
}

/// Minimal SHA-256 (no external dependencies).
fn sha256_hex(data: &[u8]) -> String {
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

#[test]
fn sha256_known_vectors() {
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

/// (sha256, name) pairs from `synthetic.sha256`.
fn expected() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(data_dir().join("synthetic.sha256")).expect("synthetic.sha256");
    let v: Vec<(String, String)> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (hash, name) = l.split_once("  ").expect("sha256sum format");
            (hash.to_string(), name.trim_end_matches('\r').to_string())
        })
        .collect();
    assert!(v.len() >= 4, "too few entries in synthetic.sha256");
    v
}

fn sqx(args: &[&Path]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_sqx")).args(args).output().expect("run sqx");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn test_command_accepts_archive() {
    let arc = data_dir().join("synthetic.sqx");
    let (ok, out) = sqx(&[Path::new("t"), &arc]);
    assert!(ok, "sqx t failed:\n{out}");
    for (_, name) in expected() {
        assert!(out.contains(&format!("OK    {name}")), "{name} not OK:\n{out}");
    }
}

#[test]
fn extract_matches_sha256() {
    let arc = data_dir().join("synthetic.sqx");
    let dir = std::env::temp_dir().join(format!("sqx-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (ok, out) = sqx(&[Path::new("x"), &arc, &dir]);
    let mut bad = Vec::new();
    for (hash, name) in expected() {
        match std::fs::read(dir.join(&name)) {
            Ok(d) if sha256_hex(&d) == hash => {}
            Ok(_) => bad.push(format!("{name}: sha256 mismatch")),
            Err(e) => bad.push(format!("{name}: {e}")),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "sqx x failed:\n{out}");
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn corrupted_archive_is_rejected() {
    let mut d = std::fs::read(data_dir().join("synthetic.sqx")).unwrap();
    // Flip a byte in the compressed data of the (large) restart.dat entry.
    let n = d.len();
    d[n - 100] ^= 0x55;
    let p = std::env::temp_dir().join(format!("sqx-e2e-bad-{}.sqx", std::process::id()));
    std::fs::write(&p, &d).unwrap();
    let (ok, out) = sqx(&[Path::new("t"), &p]);
    let _ = std::fs::remove_file(&p);
    assert!(!ok, "corruption not detected:\n{out}");
}
