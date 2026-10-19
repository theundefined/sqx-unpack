//! Extraction / testing of archive entries with length and CRC verification.

use crate::crc32::Crc32;
use crate::header::Entry;
use crate::ppmd;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Turn an archived name into a relative path that cannot escape the
/// destination directory: both separators are accepted, empty / `.` / `..`
/// components and drive prefixes are dropped, control characters replaced.
pub fn sanitize_name(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in name.split(|c| c == '/' || c == '\\') {
        let part = part.trim_matches(|c: char| c == ' ' || c == '.');
        if part.is_empty() || part.ends_with(':') && part.len() <= 2 {
            continue;
        }
        let clean: String = part.chars().map(|c| if c.is_control() || c == ':' { '_' } else { c }).collect();
        out.push(clean);
    }
    // Defence in depth: only normal components may remain.
    if out.as_os_str().is_empty() || out.components().any(|c| !matches!(c, Component::Normal(_))) {
        None
    } else {
        Some(out)
    }
}

struct CrcWriter<W: Write> {
    inner: W,
    crc: Crc32,
    len: u64,
}

impl<W: Write> Write for CrcWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.crc.update(&buf[..n]);
        self.len += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Decode one entry into `sink`, verifying size and CRC.
pub fn unpack_entry<W: Write>(archive: &[u8], e: &Entry, sink: W) -> Result<(), String> {
    if e.method != 6 {
        return Err(format!("unsupported compression method {}", e.method));
    }
    let start = e.data_offset;
    let end = usize::try_from(e.csize)
        .ok()
        .and_then(|c| start.checked_add(c))
        .filter(|&end| end <= archive.len())
        .ok_or("compressed data extends past end of archive")?;
    let mut w = CrcWriter { inner: sink, crc: Crc32::new(), len: 0 };
    ppmd::decode_sqx(&archive[start..end], e.usize, &mut w)
        .map_err(|x| x.to_string())?;
    if w.len != e.usize {
        return Err(format!("size mismatch: got {}, expected {}", w.len, e.usize));
    }
    let crc = !w.crc.finish();
    if crc != e.crc_field {
        return Err(format!("CRC mismatch: got {crc:08x}, expected {:08x}", e.crc_field));
    }
    Ok(())
}

/// Test (`dir == None`) or extract all entries.  Returns the number of failures.
pub fn run(archive: &[u8], entries: &[Entry], dir: Option<&Path>) -> usize {
    let mut failures = 0;
    for e in entries {
        let res = match dir {
            None => unpack_entry(archive, e, std::io::sink()),
            Some(d) => extract_one(archive, e, d),
        };
        match res {
            Ok(()) => println!("OK    {}", e.name),
            Err(msg) => {
                failures += 1;
                println!("FAIL  {}: {msg}", e.name);
            }
        }
    }
    failures
}

fn extract_one(archive: &[u8], e: &Entry, dir: &Path) -> Result<(), String> {
    let rel = sanitize_name(&e.name).ok_or("unsafe or empty file name")?;
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|x| x.to_string())?;
    }
    let f = std::fs::File::create(&path).map_err(|x| format!("{}: {x}", path.display()))?;
    let r = unpack_entry(archive, e, std::io::BufWriter::new(f));
    if r.is_err() {
        let _ = std::fs::remove_file(&path);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_blocks_traversal() {
        assert_eq!(sanitize_name("a/b.txt"), Some(PathBuf::from("a/b.txt")));
        assert_eq!(sanitize_name("..\\..\\etc\\passwd"), Some(PathBuf::from("etc/passwd")));
        assert_eq!(sanitize_name("/abs/../x"), Some(PathBuf::from("abs/x")));
        assert_eq!(sanitize_name("C:\\dir\\f.bak"), Some(PathBuf::from("dir/f.bak")));
        assert_eq!(sanitize_name(".."), None);
        assert_eq!(sanitize_name(""), None);
        assert_eq!(sanitize_name("a\0b"), Some(PathBuf::from("a_b")));
    }
}
