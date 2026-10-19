//! SQX method 6 decoder: PPMd variant I with a carry-less range coder.

mod alloc;
mod model;
mod rangecoder;

use model::Model;
use rangecoder::RangeDecoder;
use std::fmt;
use std::io::Write;

#[derive(Debug)]
pub enum Error {
    Corrupt(&'static str),
    Io(std::io::Error),
    BadParams(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Corrupt(m) => write!(f, "corrupt data: {m}"),
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::BadParams(m) => write!(f, "bad parameters: {m}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub order: u32,
    pub mem_size: u32,
    /// Range coder bottom limit (SQX uses `0x10000`).
    pub bot: u32,
    /// Use stock Ppmd8 start-up tables (only for cross-checking with 7-Zip's coder).
    pub ppmd8_tables: bool,
}

impl Params {
    pub fn sqx(order: u32, mem_size: u32) -> Params {
        Params { order, mem_size, bot: rangecoder::BOT, ppmd8_tables: false }
    }
}

pub const MAX_MEM: u32 = 1 << 30;

/// Decode one SQX method 6 entry stream.  The model parameters are the first
/// bits of the arithmetic-coded stream: a 1-bit flag (0 = fresh model), the
/// maximum order (8 bits) and the model memory in MB (10 bits), all coded as
/// equiprobable bits.  Exactly `out_len` symbols are decoded.
pub fn decode_sqx<W: Write>(input: &[u8], out_len: u64, w: &mut W) -> Result<(), Error> {
    let mut rc = RangeDecoder::new(input, rangecoder::BOT);
    if rc.equal_bits(1) != 0 {
        return Err(Error::BadParams("continued (solid) model streams are not supported"));
    }
    let order = rc.equal_bits(8);
    let mb = rc.equal_bits(10);
    if std::env::var("SQX_DEBUG").is_ok() { eprintln!("header: order {order} mem {mb} MB"); }
    let p = Params::sqx(order, mb << 20);
    check_params(&p)?;
    decode_with(&mut rc, p, out_len, w).map(|_| ())
}

fn check_params(p: &Params) -> Result<(), Error> {
    if p.order < 2 || p.order as usize > model::MAX_ORDER {
        return Err(Error::BadParams("order out of range"));
    }
    if p.mem_size < (1 << 11) || p.mem_size > MAX_MEM {
        return Err(Error::BadParams("memory size out of range"));
    }
    Ok(())
}

/// Decode `out_len` bytes from `input` with explicit parameters (no in-stream header).
pub fn decode<W: Write>(input: &[u8], p: Params, out_len: u64, w: &mut W) -> Result<(), Error> {
    decode_counted(input, p, out_len, w).map(|_| ())
}

/// Like [`decode`], also returns how many input bytes the range decoder consumed.
pub fn decode_counted<W: Write>(input: &[u8], p: Params, out_len: u64, w: &mut W) -> Result<usize, Error> {
    check_params(&p)?;
    let mut rc = RangeDecoder::new(input, p.bot);
    decode_with(&mut rc, p, out_len, w)
}

fn decode_with<W: Write>(rc: &mut RangeDecoder, p: Params, out_len: u64, w: &mut W) -> Result<usize, Error> {
    let mut model = Model::new_with(p.mem_size, p.order, p.ppmd8_tables);
    let mut buf = vec![0u8; 1 << 16];
    let mut left = out_len;
    while left > 0 {
        let n = left.min(buf.len() as u64) as usize;
        for k in 0..n {
            match model.decode_symbol(rc) {
                Ok(b) => buf[k] = b,
                Err(e) => {
                    // Deliver what was decoded before the error.
                    w.write_all(&buf[..k])?;
                    return Err(e);
                }
            }
        }
        if rc.overrun() {
            return Err(Error::Corrupt("input truncated"));
        }
        w.write_all(&buf[..n])?;
        left -= n as u64;
    }
    Ok(rc.consumed())
}

