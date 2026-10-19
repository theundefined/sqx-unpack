//! Unit tests on synthetic data.  A test-only encoder mirrors the decoder so
//! that the model (including memory exhaustion and restarts) can be exercised
//! without any real archive.

use super::*;
use crate::ppmd::rangecoder::{RangeEncoder, BOT};

fn encode(data: &[u8], order: u32, mem: u32) -> Vec<u8> {
    let mut m = Model::new(mem, order);
    let mut rc = RangeEncoder::new(BOT);
    for &b in data {
        m.encode_symbol(&mut rc, b);
    }
    rc.finish()
}

fn decode(stream: &[u8], n: usize, order: u32, mem: u32) -> Vec<u8> {
    let mut m = Model::new(mem, order);
    let mut rc = RangeDecoder::new(stream, BOT);
    (0..n).map(|_| m.decode_symbol(&mut rc).unwrap()).collect()
}

/// Small deterministic generator (xorshift) for synthetic input.
fn synth(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let words: Vec<Vec<u8>> = (0..300)
        .map(|_| (0..2 + next() % 8).map(|_| b'a' + (next() % 26) as u8).collect())
        .collect();
    let mut out = Vec::with_capacity(len + 16);
    while out.len() < len {
        let r = next();
        if r % 17 == 0 {
            out.push((r >> 8) as u8); // occasional random byte
        } else {
            out.extend_from_slice(&words[(r >> 8) as usize % words.len()]);
            out.push(b' ');
        }
    }
    out.truncate(len);
    out
}

#[test]
fn roundtrip_text_like() {
    let d = synth(60_000, 1);
    let c = encode(&d, 8, 1 << 20);
    assert!(c.len() < d.len() / 2);
    assert_eq!(decode(&c, d.len(), 8, 1 << 20), d);
}

#[test]
fn roundtrip_with_model_restarts() {
    let d = synth(150_000, 2);
    for &(order, mem) in &[(6u32, 1u32 << 16), (8, 40_000), (3, 4096), (16, 1 << 15)] {
        let c = encode(&d, order, mem);
        assert_eq!(decode(&c, d.len(), order, mem), d, "order {order} mem {mem}");
    }
}

#[test]
fn roundtrip_binary_and_runs() {
    let mut d = vec![0u8; 5000];
    d.extend((0..20_000u32).map(|i| (i * i >> 3) as u8));
    d.extend(std::iter::repeat(0xAA).take(7000));
    let c = encode(&d, 8, 1 << 18);
    assert_eq!(decode(&c, d.len(), 8, 1 << 18), d);
}

#[test]
fn corrupt_input_does_not_panic() {
    let mut x = 12345u64;
    for _ in 0..20 {
        let junk: Vec<u8> = (0..400)
            .map(|_| {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (x >> 33) as u8
            })
            .collect();
        let mut m = Model::new(1 << 16, 8);
        let mut rc = RangeDecoder::new(&junk, BOT);
        for _ in 0..3000 {
            if m.decode_symbol(&mut rc).is_err() || rc.overrun() {
                break;
            }
        }
    }
}

/// Stream produced by an independent PPMd var.I implementation (7-Zip's
/// Ppmd8 coder, bottom limit 2^15) for 48 distinct bytes: only the coder and
/// the order-0 model are exercised, which are known to be compatible.
#[test]
fn matches_reference_stream_for_new_symbols() {
    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }
    let plain = unhex("2163df470f523b8925c08b2ce8bd0e67bee3692342b9e050385f4856cbec3e974372e6c5d1545149afca4f5d73343d5b");
    let mut stream = unhex("21449d3ae386ac9e091f451e2a2e1007e9337de35c3f6fc19f95b52b3ad04d60d6be35124261ac4f9d887065d4a7ca518fa92047b90000");
    while stream.len() % 4 != 0 {
        stream.push(0);
    }
    for w in stream.chunks_mut(4) {
        w.reverse(); // our decoder expects word-reversed storage
    }
    let mut m = Model::new(1 << 16, 8);
    let mut rc = RangeDecoder::new(&stream, 1 << 15);
    let got: Vec<u8> = (0..plain.len()).map(|_| m.decode_symbol(&mut rc).unwrap()).collect();
    assert_eq!(got, plain);
}

/// Throughput check, run with `cargo test --release -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_decode_throughput() {
    let d = synth(std::env::var("BN").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000_000), 3);
    let c = encode(&d, 8, 32 << 20);
    let t = std::time::Instant::now();
    let out = decode(&c, d.len(), 8, 32 << 20);
    let secs = t.elapsed().as_secs_f64();
    assert_eq!(out, d);
    println!("decoded {} bytes ({} compressed) at {:.1} MB/s", d.len(), c.len(), d.len() as f64 / secs / 1e6);
}

#[test]
#[ignore]
fn bench_variants() {
    let easy: Vec<u8> = (0..8_000_000usize).map(|i| b"hello world, this is a test. "[i % 29]).collect();
    let mut x = 7u64;
    let rnd: Vec<u8> = (0..2_000_000).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x as u8 }).collect();
    for (name, d) in [("easy", &easy), ("random", &rnd)] {
        let c = encode(d, 8, 32 << 20);
        let t = std::time::Instant::now();
        let out = decode(&c, d.len(), 8, 32 << 20);
        let secs = t.elapsed().as_secs_f64();
        assert_eq!(&out, d);
        println!("{name}: {:.1} MB/s", d.len() as f64 / secs / 1e6);
    }
}
