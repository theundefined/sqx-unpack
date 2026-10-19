//! CRC-32 (IEEE 802.3, reflected, as in zip/gzip) with a slicing-by-8 table.

const fn make_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut j = 1;
        while j < 8 {
            let p = t[j - 1][i];
            t[j][i] = (p >> 8) ^ t[0][(p & 0xFF) as usize];
            j += 1;
        }
        i += 1;
    }
    t
}

static TABLES: [[u32; 256]; 8] = make_tables();

#[derive(Clone, Copy)]
pub struct Crc32(u32);

impl Crc32 {
    pub fn new() -> Self {
        Crc32(0xFFFF_FFFF)
    }
    pub fn update(&mut self, data: &[u8]) {
        let t = &TABLES;
        let mut c = self.0;
        let mut chunks = data.chunks_exact(8);
        for ch in &mut chunks {
            let a = u32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]) ^ c;
            let b = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]);
            c = t[7][(a & 0xFF) as usize]
                ^ t[6][((a >> 8) & 0xFF) as usize]
                ^ t[5][((a >> 16) & 0xFF) as usize]
                ^ t[4][(a >> 24) as usize]
                ^ t[3][(b & 0xFF) as usize]
                ^ t[2][((b >> 8) & 0xFF) as usize]
                ^ t[1][((b >> 16) & 0xFF) as usize]
                ^ t[0][(b >> 24) as usize];
        }
        for &b in chunks.remainder() {
            c = (c >> 8) ^ t[0][((c ^ b as u32) & 0xFF) as usize];
        }
        self.0 = c;
    }
    /// Final CRC value (standard CRC-32).
    pub fn finish(&self) -> u32 {
        !self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn check_value() {
        let mut c = Crc32::new();
        c.update(b"123456789");
        assert_eq!(c.finish(), 0xCBF4_3926);
    }
}
