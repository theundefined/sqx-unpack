//! Parsing of SQX container headers. See docs/SQX-format.md.

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub method: u8,
    pub crc_field: u32,
    pub attr: u32,
    pub mtime: u32,
    pub csize: u64,
    pub usize: u64,
    pub data_offset: usize,
}

fn u16le(d: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(p..p + 2)?.try_into().ok()?))
}
fn u32le(d: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(p..p + 4)?.try_into().ok()?))
}
fn u64le(d: &[u8], p: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(p..p + 8)?.try_into().ok()?))
}

/// Skip extension headers chained by flag 0x8000, returning the offset after them.
fn skip_ext(d: &[u8], mut p: usize, mut flags: u16) -> Option<usize> {
    while flags & 0x8000 != 0 {
        flags = u16le(d, p + 3)?;
        let sz = u16le(d, p + 5)? as usize;
        if sz < 7 {
            return None;
        }
        p += sz;
    }
    Some(p)
}

pub fn parse(d: &[u8]) -> Result<Vec<Entry>, String> {
    let sig = d
        .windows(5)
        .position(|w| w == b"-sqx-")
        .ok_or("not an SQX archive (no -sqx- signature)")?;
    let mut pos = sig.checked_sub(7).ok_or("bad main header")?;
    pos += u16le(d, pos + 5).ok_or("truncated main header")? as usize;
    let mut out = Vec::new();
    while pos + 7 <= d.len() {
        let typ = d[pos + 2];
        let flags = u16le(d, pos + 3).unwrap();
        let hsz = u16le(d, pos + 5).unwrap() as usize;
        if typ == b'X' || hsz < 7 {
            break;
        }
        let after = skip_ext(d, pos + hsz, flags).ok_or("bad extension header chain")?;
        if typ == b'D' {
            let h = d.get(pos..pos + hsz).ok_or("truncated entry header")?;
            let t = || "truncated entry header";
            let (csize, usize_, nl, np) = if flags & 0x80 != 0 {
                (u64le(h, 0x19).ok_or_else(t)?, u64le(h, 0x21).ok_or_else(t)?, u16le(h, 0x29).ok_or_else(t)? as usize, 0x2b)
            } else {
                (u32le(h, 0x19).ok_or_else(t)? as u64, u32le(h, 0x1d).ok_or_else(t)? as u64, u16le(h, 0x21).ok_or_else(t)? as usize, 0x23)
            };
            let raw = h.get(np..np + nl).ok_or_else(t)?;
            let name: String = raw.iter().map(|&b| b as char).collect(); // TODO: code page
            out.push(Entry {
                name,
                method: *h.get(12).ok_or_else(t)?,
                crc_field: u32le(h, 13).ok_or_else(t)?,
                attr: u32le(h, 0x11).ok_or_else(t)?,
                mtime: u32le(h, 0x15).ok_or_else(t)?,
                csize,
                usize: usize_,
                data_offset: after,
            });
            pos = after.checked_add(csize as usize).ok_or("size overflow")?;
        } else {
            pos = after;
        }
    }
    Ok(out)
}
