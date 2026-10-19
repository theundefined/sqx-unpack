//! Dev tool: decode streams produced by a reference Ppmd8 encoder (BOT = 2^15)
//! and compare. usage: refcheck NAME ORDER MEM  (reads NAME.raw / NAME.cmp)
use sqx_unpack::ppmd;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let raw = std::fs::read(format!("{}.raw", a[1])).unwrap();
    let mut cmp = std::fs::read(format!("{}.cmp", a[1])).unwrap();
    while cmp.len() % 4 != 0 { cmp.push(0); }
    for w in cmp.chunks_mut(4) { w.reverse(); }
    let mut out = Vec::new();
    let r = ppmd::decode(&cmp, ppmd::Params { order: a[2].parse().unwrap(), mem_size: a[3].parse().unwrap(), bot: 1 << 15, ppmd8_tables: true }, raw.len() as u64, &mut out);
    let m = out.iter().zip(raw.iter()).position(|(x, y)| x != y).unwrap_or(raw.len().min(out.len()));
    println!("{} {}", if out == raw { "OK" } else { "FAIL" }, if r.is_err() { "err" } else { "" });
    let _ = m;
}
