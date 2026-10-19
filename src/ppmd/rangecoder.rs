//! Carry-less range decoder used by SQX method 6 (Subbotin style).
//!
//! The coder keeps a 32-bit interval `[low, low + range)` and a 32-bit `code`
//! window of the compressed stream.  To decode a symbol with cumulative
//! frequency interval `[start, start + size)` out of `total`, the decoder first
//! divides the range by `total` (so one frequency unit is `range` wide), looks
//! at `(code - low) / range` to find which unit the code falls in, and then
//! narrows the interval to the chosen sub-interval.
//!
//! After narrowing, the interval is renormalised: while the top byte of `low`
//! and of `low + range` agree (`(low ^ (low + range)) < 2^24`) that byte can
//! never change any more and is shifted out.  If the interval became too small
//! (`range < BOT`) while straddling a byte boundary, it is artificially
//! shrunk to end at the boundary (`range = -low & (BOT - 1)`) so no carry
//! can ever occur.  The encoder performs exactly the same steps.
//!
//! The compressed stream is consumed as little-endian 32-bit words whose bytes
//! are handed to the coder from the most significant byte down, i.e. within
//! each aligned group of four bytes the order is reversed.

use super::Error;

const TOP: u32 = 1 << 24;
/// Default bottom limit of SQX; variants are used for cross-checking.
pub const BOT: u32 = 1 << 16;

pub struct RangeDecoder<'a> {
    data: &'a [u8],
    /// Number of stream bytes consumed so far.
    pos: usize,
    low: u32,
    code: u32,
    range: u32,
    bot: u32,
}

impl<'a> RangeDecoder<'a> {
    pub fn new(data: &'a [u8], bot: u32) -> RangeDecoder<'a> {
        let mut rc = RangeDecoder { data, pos: 0, low: 0, code: 0, range: u32::MAX, bot };
        for _ in 0..4 {
            rc.code = (rc.code << 8) | rc.next_byte() as u32;
        }
        rc
    }

    /// Next byte of the stream after undoing the per-word byte reversal.
    /// Bytes past the end read as zero (the last word is zero-extended).
    #[inline]
    fn next_byte(&mut self) -> u8 {
        let i = self.pos;
        self.pos += 1;
        let idx = (i & !3) | (3 - (i & 3));
        self.data.get(idx).copied().unwrap_or(0)
    }

    /// Decode an `n`-bit value (MSB first) made of equiprobable bits.
    pub fn equal_bits(&mut self, n: u32) -> u32 {
        let mut v = 0;
        for _ in 0..n {
            let q = self.threshold(2).unwrap_or(0).min(1);
            self.decode(q, 1);
            v = (v << 1) | q;
        }
        v
    }

    /// Number of stream bytes consumed so far.
    pub fn consumed(&self) -> usize {
        self.pos
    }

    /// True if the decoder has read more than a few bytes beyond the input.
    pub fn overrun(&self) -> bool {
        self.pos > self.data.len() + 8
    }

    /// Scale the range to `total` units and return the unit the code falls in.
    /// The result may be >= `total` on corrupt input; callers must check.
    #[inline]
    pub fn threshold(&mut self, total: u32) -> Result<u32, Error> {
        if total == 0 {
            return Err(Error::Corrupt("zero total frequency"));
        }
        self.range /= total;
        if self.range == 0 {
            return Err(Error::Corrupt("range underflow"));
        }
        Ok(self.code.wrapping_sub(self.low) / self.range)
    }

    /// Narrow the interval to `[start, start + size)` (in units established by
    /// the preceding `threshold`) and renormalise.
    #[inline]
    pub fn decode(&mut self, start: u32, size: u32) {
        self.low = self.low.wrapping_add(start.wrapping_mul(self.range));
        self.range = self.range.wrapping_mul(size);
        loop {
            if (self.low ^ self.low.wrapping_add(self.range)) >= TOP {
                if self.range >= self.bot {
                    break;
                }
                self.range = self.low.wrapping_neg() & (self.bot - 1);
            }
            self.code = (self.code << 8) | self.next_byte() as u32;
            self.low <<= 8;
            self.range <<= 8;
            if self.pos > self.data.len() + 64 {
                // Hopelessly past the end of input (also guards against a
                // zero range looping forever); the caller checks `overrun`.
                break;
            }
        }
    }
}

/// Reference encoder, only used by the unit tests to produce synthetic streams.
#[cfg(test)]
pub struct RangeEncoder {
    low: u32,
    range: u32,
    out: Vec<u8>,
    bot: u32,
}

#[cfg(test)]
impl RangeEncoder {
    pub fn new(bot: u32) -> Self {
        RangeEncoder { low: 0, range: u32::MAX, out: Vec::new(), bot }
    }
    pub fn scale(&mut self, total: u32) {
        self.range /= total;
    }
    pub fn encode(&mut self, start: u32, size: u32) {
        self.low = self.low.wrapping_add(start.wrapping_mul(self.range));
        self.range = self.range.wrapping_mul(size);
        loop {
            if (self.low ^ self.low.wrapping_add(self.range)) >= TOP {
                if self.range >= self.bot {
                    break;
                }
                self.range = self.low.wrapping_neg() & (self.bot - 1);
            }
            self.out.push((self.low >> 24) as u8);
            self.low <<= 8;
            self.range <<= 8;
        }
    }
    /// Flush and return the stream in the word-reversed storage order.
    pub fn finish(mut self) -> Vec<u8> {
        for _ in 0..4 {
            self.out.push((self.low >> 24) as u8);
            self.low <<= 8;
        }
        while self.out.len() % 4 != 0 {
            self.out.push(0);
        }
        for w in self.out.chunks_mut(4) {
            w.reverse();
        }
        self.out
    }
}
