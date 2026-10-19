//! PPMd variant I context model (statistics, escape estimation, updates).
//!
//! Terminology.  A *context* is the last few already-seen bytes.  For every
//! context the model keeps the list of bytes that have followed it with a
//! frequency each.  Contexts are chained: the *suffix* of a context is the
//! same context with its oldest byte dropped (the order-0 root context has
//! none).  To code a byte, the longest known context is tried first; if the
//! byte is not in its list an *escape* is coded and the next shorter context
//! is tried, with the bytes already ruled out masked away.
//!
//! Memory layout inside the arena (little endian, offsets are 32-bit):
//!
//! ```text
//! context (12 bytes): num_stats-1 : u8   (0 => binary context, one symbol)
//!                     flags       : u8
//!                     summ_freq   : u16
//!                     stats       : u32  -> array of states
//!                     suffix      : u32
//! state (6 bytes):    symbol u8, freq u8, successor u32
//! ```
//!
//! A binary context stores its only state inline, overlaying the `summ_freq`
//! and `stats` fields (offset 2).  A state's *successor* is either a context
//! (offset >= units_start) or a position inside the text area (a context that
//! has not been created yet), or 0.

use super::alloc::{Arena, NUM_INDEXES, UNIT_SIZE};
use super::rangecoder::RangeDecoder;
use super::Error;

pub const MAX_ORDER: usize = 64;
const MAX_FREQ: u32 = 124;
const INT_BITS: u32 = 7;
const PERIOD_BITS: u32 = 7;
pub(super) const BIN_SCALE: u32 = 1 << (INT_BITS + PERIOD_BITS);

const INIT_BIN_ESC: [u16; 8] = [0x3CDD, 0x1F3F, 0x59BF, 0x48F3, 0x64A1, 0x5ABC, 0x6632, 0x6051];
const EXP_ESCAPE: [u8; 16] = [25, 14, 9, 7, 5, 5, 4, 4, 4, 3, 3, 3, 2, 2, 2, 2];

/// SQX start-up divisors of the binary-context probabilities, per frequency
/// class (largest frequency of the class + 1).
const SQX_BIN_DIV: [u32; 25] =
    [2, 3, 4, 5, 6, 7, 9, 12, 16, 21, 27, 34, 42, 51, 61, 72, 84, 97, 111, 126, 142, 159, 177, 196, 216];
/// SQX start-up SEE bases per row (largest NumStats of the class; last row 255).
const SQX_SEE_P: [u32; 24] =
    [1, 2, 3, 5, 8, 12, 17, 23, 30, 38, 47, 57, 68, 80, 93, 107, 122, 138, 155, 173, 192, 212, 233, 255];
const STATE: u32 = 6;
const DUMMY_SEE: usize = 24 * 32;

#[derive(Clone, Copy, Default)]
struct See {
    summ: u16,
    shift: u8,
    count: u8,
}

impl See {
    /// Adaptive escape estimator update after a successful (non-escape) coding.
    #[inline]
    fn update(&mut self) {
        if (self.shift as u32) < PERIOD_BITS {
            self.count = self.count.wrapping_sub(1);
            if self.count == 0 {
                self.summ = self.summ.wrapping_shl(1);
                self.count = (3u32 << self.shift) as u8;
                self.shift += 1;
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct StateVal {
    sym: u8,
    freq: u8,
    succ: u32,
}

pub struct Model {
    pub(super) a: Arena,
    min_context: u32,
    max_context: u32,
    pub(super) found_state: u32,
    order_fall: u32,
    init_esc: u32,
    prev_success: u32,
    max_order: u32,
    /// Use the stock Ppmd8 start-up tables instead of the SQX ones.
    ppmd8_tables: bool,
    run_length: i32,
    init_rl: i32,
    ns2bsindx: [u8; 256],
    ns2indx: [u8; 260],
    see: Vec<See>,
    bin_summ: [[u16; 64]; 25],
    /// Symbols already excluded in the current escape sequence are marked
    /// with the current `esc_count` (avoids clearing 256 bytes per symbol).
    char_mask: [u8; 256],
    esc_count: u8,
    ps: [u32; 256],
    ps_sym: [u8; 256],
    ps_freq: [u8; 256],
}

impl Model {
    /// `mem_size` bytes of model memory, contexts up to `max_order` bytes.
    #[cfg(any(test, feature = "encoder"))]
    pub fn new(mem_size: u32, max_order: u32) -> Model {
        Model::new_with(mem_size, max_order, false)
    }

    /// `ppmd8_tables = true` selects the stock 7-Zip Ppmd8 start-up tables.
    pub fn new_with(mem_size: u32, max_order: u32, ppmd8_tables: bool) -> Model {
        let mut ns2bsindx = [0u8; 256];
        ns2bsindx[0] = 0;
        ns2bsindx[1] = 2;
        for v in &mut ns2bsindx[2..11] {
            *v = 4;
        }
        for v in &mut ns2bsindx[11..] {
            *v = 6;
        }
        let mut ns2indx = [0u8; 260];
        for i in 0..5 {
            ns2indx[i] = i as u8;
        }
        let (mut m, mut k) = (5u32, 1u32);
        for i in 5..260 {
            ns2indx[i] = m as u8;
            k -= 1;
            if k == 0 {
                m += 1;
                k = m - 4;
            }
        }
        let mut md = Model {
            a: Arena::new(mem_size),
            min_context: 0,
            max_context: 0,
            found_state: 0,
            order_fall: 0,
            init_esc: 0,
            prev_success: 0,
            max_order,
            ppmd8_tables,
            run_length: 0,
            init_rl: 0,
            ns2bsindx,
            ns2indx,
            see: vec![See::default(); DUMMY_SEE + 1],
            bin_summ: [[0; 64]; 25],
            char_mask: [0; 256],
            esc_count: 0,
            ps: [0; 256],
            ps_sym: [0; 256],
            ps_freq: [0; 256],
        };
        md.restart();
        md.see[DUMMY_SEE] = See { summ: 0, shift: PERIOD_BITS as u8, count: 64 };
        md
    }

    // ---- field accessors ---------------------------------------------------

    #[inline(always)]
    fn ns(&self, c: u32) -> u32 {
        self.a.rd8(c) as u32
    }
    #[inline(always)]
    fn flags(&self, c: u32) -> u8 {
        self.a.rd8(c + 1)
    }
    #[inline(always)]
    fn summ(&self, c: u32) -> u32 {
        self.a.rd16(c + 2) as u32
    }
    #[inline(always)]
    fn set_summ(&mut self, c: u32, v: u32) {
        self.a.wr16(c + 2, v as u16);
    }
    #[inline(always)]
    fn stats(&self, c: u32) -> u32 {
        self.a.rd32(c + 4)
    }
    #[inline(always)]
    fn suffix(&self, c: u32) -> u32 {
        self.a.rd32(c + 8)
    }
    #[inline(always)]
    fn sym(&self, s: u32) -> u8 {
        self.a.rd8(s)
    }
    #[inline(always)]
    fn freq(&self, s: u32) -> u32 {
        self.a.rd8(s + 1) as u32
    }
    #[inline(always)]
    fn set_freq(&mut self, s: u32, f: u32) {
        self.a.wr8(s + 1, f as u8);
    }
    #[inline(always)]
    fn succ(&self, s: u32) -> u32 {
        self.a.rd32(s + 2)
    }
    #[inline(always)]
    fn set_succ(&mut self, s: u32, v: u32) {
        self.a.wr32(s + 2, v);
    }
    #[inline(always)]
    fn get_state(&self, s: u32) -> StateVal {
        StateVal { sym: self.sym(s), freq: self.a.rd8(s + 1), succ: self.succ(s) }
    }
    #[inline(always)]
    fn put_state(&mut self, s: u32, v: StateVal) {
        self.a.wr8(s, v.sym);
        self.a.wr8(s + 1, v.freq);
        self.a.wr32(s + 2, v.succ);
    }
    fn swap_states(&mut self, x: u32, y: u32) {
        let vx = self.get_state(x);
        let vy = self.get_state(y);
        self.put_state(x, vy);
        self.put_state(y, vx);
    }
    /// Locate `sym` in the state array of multi-symbol context `c`.
    #[inline]
    fn find_state(&self, c: u32, sym: u8) -> u32 {
        let mut s = self.stats(c);
        let end = s + self.ns(c) * STATE;
        while self.sym(s) != sym && s < end {
            s += STATE;
        }
        s
    }

    // ---- (re)initialisation ------------------------------------------------

    /// Reset the model to the initial state: order-0 context with all 256
    /// symbols at frequency 1.
    fn restart(&mut self) {
        self.a.reset();
        self.order_fall = self.max_order;
        self.init_rl = -(self.max_order.min(12) as i32) - 1;
        self.run_length = self.init_rl;
        self.prev_success = 0;

        self.a.hi_unit -= UNIT_SIZE;
        let root = self.a.hi_unit;
        self.min_context = root;
        self.max_context = root;
        self.a.wr32(root + 8, 0);
        self.a.wr8(root, 255);
        self.a.wr8(root + 1, 0);
        self.set_summ(root, 256 + 1);
        let stats = self.a.lo_unit;
        self.found_state = stats;
        self.a.lo_unit += (256 / 2) * UNIT_SIZE;
        self.a.wr32(root + 4, stats);
        for i in 0..256u32 {
            let s = stats + i * STATE;
            self.put_state(s, StateVal { sym: i as u8, freq: 1, succ: 0 });
        }
        if self.ppmd8_tables {
            // Stock Ppmd8: divisor / SEE base from the first index past the class.
            let mut i = 0usize;
            for m in 0..25usize {
                while self.ns2indx[i] as usize == m {
                    i += 1;
                }
                self.init_bin_row(m, i as u32 + 1);
            }
            let mut i = 0usize;
            for m in 0..24usize {
                while self.ns2indx[i + 3] as usize == m + 3 {
                    i += 1;
                }
                self.init_see_row(m, i as u32);
            }
        } else {
            for m in 0..25usize {
                self.init_bin_row(m, SQX_BIN_DIV[m]);
            }
            for m in 0..24usize {
                self.init_see_row(m, SQX_SEE_P[m]);
            }
        }
    }

    fn init_bin_row(&mut self, row: usize, div: u32) {
        for k in 0..8usize {
            let val = (BIN_SCALE - INIT_BIN_ESC[k] as u32 / div) as u16;
            for r in (0..64).step_by(8) {
                self.bin_summ[row][k + r] = val;
            }
        }
    }

    fn init_see_row(&mut self, row: usize, p: u32) {
        for k in 0..32usize {
            self.see[row * 32 + k] = See {
                summ: ((2 * p + 5) << (PERIOD_BITS - 4)) as u16,
                shift: (PERIOD_BITS - 4) as u8,
                count: 7,
            };
        }
    }

    // ---- model update ------------------------------------------------------

    /// Build the chain of missing contexts for the symbol just coded.
    /// Returns the new (longest) context, or 0 when memory ran out.
    fn create_successors(&mut self, skip: bool, mut s1: u32, mut c: u32) -> u32 {
        let fs = self.found_state;
        let fsym = self.sym(fs);
        let up_branch = self.succ(fs);
        let mut ps = [0u32; MAX_ORDER + 1];
        let mut num_ps = 0usize;
        if !skip {
            ps[num_ps] = fs;
            num_ps += 1;
        }
        while self.suffix(c) != 0 {
            c = self.suffix(c);
            let s;
            if s1 != 0 {
                s = s1;
                s1 = 0;
            } else if self.ns(c) != 0 {
                s = self.find_state(c, fsym);
                if self.freq(s) < MAX_FREQ - 9 {
                    self.set_freq(s, self.freq(s) + 1);
                    let sf = self.summ(c) + 1;
                    self.set_summ(c, sf);
                }
            } else {
                s = c + 2;
                let inc = (self.ns(self.suffix(c)) == 0 && self.freq(s) < 24) as u32;
                self.set_freq(s, self.freq(s) + inc);
            }
            let successor = self.succ(s);
            if successor != up_branch {
                c = successor;
                if num_ps == 0 {
                    return c;
                }
                break;
            }
            if num_ps >= ps.len() {
                return 0;
            }
            ps[num_ps] = s;
            num_ps += 1;
        }
        let up_sym = self.a.rd8(up_branch);
        let up_succ = up_branch + 1;
        let flags = 0x10 * (fsym >= 0x40) as u8 + 0x08 * (up_sym >= 0x40) as u8;
        let up_freq: u32 = if self.ns(c) == 0 {
            self.freq(c + 2)
        } else {
            let s = self.find_state(c, up_sym);
            let cf = self.freq(s) - 1;
            let s0 = self.summ(c) - self.ns(c) - cf;
            1 + if 2 * cf <= s0 { (5 * cf > s0) as u32 } else { (cf + 2 * s0 - 3) / s0 }
        };
        while num_ps != 0 {
            let c1 = self.a.alloc_context();
            if c1 == 0 {
                return 0;
            }
            self.a.wr8(c1, 0);
            self.a.wr8(c1 + 1, flags);
            self.put_state(c1 + 2, StateVal { sym: up_sym, freq: up_freq as u8, succ: up_succ });
            self.a.wr32(c1 + 8, c);
            num_ps -= 1;
            self.set_succ(ps[num_ps], c1);
            c = c1;
        }
        c
    }

    /// Find or build the context to continue with when no suffix could be
    /// determined cheaply (used when the found state has no successor).
    fn reduce_order(&mut self, mut s1: u32, mut c: u32) -> u32 {
        let c1 = c;
        let up_branch = self.a.text;
        let fs = self.found_state;
        let fsym = self.sym(fs);
        self.set_succ(fs, up_branch);
        self.order_fall += 1;
        let mut s;
        loop {
            if s1 != 0 {
                c = self.suffix(c);
                s = s1;
                s1 = 0;
            } else {
                if self.suffix(c) == 0 {
                    return c;
                }
                c = self.suffix(c);
                if self.ns(c) != 0 {
                    s = self.find_state(c, fsym);
                    if self.freq(s) < MAX_FREQ - 9 {
                        self.set_freq(s, self.freq(s) + 2);
                        let sf = self.summ(c) + 2;
                        self.set_summ(c, sf);
                    }
                } else {
                    s = c + 2;
                    let inc = (self.freq(s) < 32) as u32;
                    self.set_freq(s, self.freq(s) + inc);
                }
            }
            if self.succ(s) != 0 {
                break;
            }
            self.set_succ(s, up_branch);
            self.order_fall += 1;
        }
        if self.succ(s) <= up_branch {
            let s2 = self.found_state;
            self.found_state = s;
            let successor = self.create_successors(false, 0, c);
            self.set_succ(s, successor);
            self.found_state = s2;
        }
        if self.order_fall == 1 && c1 == self.max_context {
            let v = self.succ(s);
            self.set_succ(self.found_state, v);
            self.a.text -= 1;
        }
        self.succ(s)
    }

    /// Memory exhausted: throw the whole model away and start afresh.
    fn restore_model(&mut self) {
        self.restart();
    }

    /// Add the coded symbol to all contexts it was missing from, create the
    /// next contexts, and set `max_context`/`min_context` for the next symbol.
    fn update_model(&mut self) {
        let fs = self.found_state;
        let mut f_successor = self.succ(fs);
        let f_freq = self.freq(fs);
        let f_symbol = self.sym(fs);
        let mut s = 0u32;

        if f_freq < MAX_FREQ / 4 && self.suffix(self.min_context) != 0 {
            let c = self.suffix(self.min_context);
            if self.ns(c) == 0 {
                s = c + 2;
                if self.freq(s) < 32 {
                    self.set_freq(s, self.freq(s) + 1);
                }
            } else {
                s = self.stats(c);
                if self.sym(s) != f_symbol {
                    s = self.find_state(c, f_symbol);
                    if self.freq(s) >= self.freq(s - STATE) {
                        self.swap_states(s, s - STATE);
                        s -= STATE;
                    }
                }
                if self.freq(s) < MAX_FREQ - 9 {
                    self.set_freq(s, self.freq(s) + 2);
                    let sf = self.summ(c) + 2;
                    self.set_summ(c, sf);
                }
            }
        }

        let c_top = self.max_context;
        if self.order_fall == 0 && f_successor != 0 {
            let cs = self.create_successors(true, s, self.min_context);
            if cs == 0 {
                self.set_succ(self.found_state, 0);
                self.restore_model();
            } else {
                self.set_succ(self.found_state, cs);
                self.max_context = cs;
            }
            return;
        }

        self.a.wr8(self.a.text, f_symbol);
        self.a.text += 1;
        let mut successor = self.a.text;
        if self.a.text >= self.a.units_start {
            self.restore_model();
            return;
        }

        if f_successor == 0 {
            let cs = self.reduce_order(s, self.min_context);
            if cs == 0 {
                self.restore_model();
                return;
            }
            f_successor = cs;
        } else if f_successor < self.a.units_start {
            let cs = self.create_successors(false, s, self.min_context);
            if cs == 0 {
                self.restore_model();
                return;
            }
            f_successor = cs;
        }

        self.order_fall -= 1;
        if self.order_fall == 0 {
            successor = f_successor;
            if self.max_context != self.min_context {
                self.a.text -= 1;
            }
        }

        let ns = self.ns(self.min_context);
        let s0 = self.summ(self.min_context) - ns - f_freq;
        let flag = 0x08 * (f_symbol >= 0x40) as u8;

        let mut c = c_top;
        while c != self.min_context {
            let ns1 = self.ns(c);
            if ns1 != 0 {
                if ns1 & 1 != 0 {
                    // The state array is full (odd count fills its last
                    // unit exactly): grow by one unit.
                    let old_nu = (ns1 + 1) >> 1;
                    let i = self.a.u2i(old_nu);
                    if i != self.a.u2i(old_nu + 1) {
                        let ptr = self.a.alloc_units(i + 1);
                        if ptr == 0 {
                            self.restore_model();
                            return;
                        }
                        let old_ptr = self.stats(c);
                        self.a.copy_units(ptr, old_ptr, old_nu);
                        self.a.insert_node(old_ptr, i);
                        self.a.wr32(c + 4, ptr);
                    }
                }
                let add = (3 * ns1 + 1 < ns) as u32;
                let sf = self.summ(c) + add;
                self.set_summ(c, sf);
            } else {
                let st = self.a.alloc_units(0);
                if st == 0 {
                    self.restore_model();
                    return;
                }
                let mut v = self.get_state(c + 2);
                v.freq = if (v.freq as u32) < MAX_FREQ / 4 - 1 { v.freq << 1 } else { (MAX_FREQ - 4) as u8 };
                self.put_state(st, v);
                self.a.wr32(c + 4, st);
                let sf = v.freq as u32 + self.init_esc + (ns > 2) as u32;
                self.set_summ(c, sf);
            }
            let mut cf = 2 * f_freq * (self.summ(c) + 6);
            let sf = s0 + self.summ(c);
            if cf < 6 * sf {
                cf = 1 + (cf > sf) as u32 + (cf >= 4 * sf) as u32;
                let t = self.summ(c) + 4;
                self.set_summ(c, t);
            } else {
                cf = 4 + (cf > 9 * sf) as u32 + (cf > 12 * sf) as u32 + (cf > 15 * sf) as u32;
                let t = self.summ(c) + cf;
                self.set_summ(c, t);
            }
            let ns_new = self.stats(c) + (ns1 + 1) * STATE;
            self.put_state(ns_new, StateVal { sym: f_symbol, freq: cf as u8, succ: successor });
            let fl = self.flags(c) | flag;
            self.a.wr8(c + 1, fl);
            self.a.wr8(c, (ns1 + 1) as u8);
            c = self.suffix(c);
        }
        self.max_context = f_successor;
        self.min_context = f_successor;
    }

    /// Halve all frequencies of the current context (called on overflow) and
    /// drop symbols that fall to zero.
    fn rescale(&mut self) {
        let mc = self.min_context;
        let stats = self.stats(mc);
        let mut s = self.found_state;
        if s != stats {
            let tmp = self.get_state(s);
            self.a.mem.copy_within(stats as usize..s as usize, (stats + STATE) as usize);
            self.put_state(stats, tmp);
            s = stats;
        }
        let mut esc_freq = self.summ(mc).wrapping_sub(self.freq(s));
        let f = self.freq(s) + 4;
        let adder = (self.order_fall != 0) as u32;
        let f = (f + adder) >> 1;
        self.set_freq(s, f);
        let mut sum_freq = f;
        let mut i = self.ns(mc);
        loop {
            s += STATE;
            esc_freq = esc_freq.wrapping_sub(self.freq(s));
            let f = (self.freq(s) + adder) >> 1;
            self.set_freq(s, f);
            sum_freq += f;
            if f > self.freq(s - STATE) {
                let tmp = self.get_state(s);
                let mut s1 = s;
                loop {
                    let prev = self.get_state(s1 - STATE);
                    self.put_state(s1, prev);
                    s1 -= STATE;
                    if s1 == stats || tmp.freq as u32 <= self.freq(s1 - STATE) {
                        break;
                    }
                }
                self.put_state(s1, tmp);
            }
            i -= 1;
            if i == 0 {
                break;
            }
        }
        if self.freq(s) == 0 {
            let num_stats = self.ns(mc);
            let mut removed = 0u32;
            loop {
                removed += 1;
                s -= STATE;
                if self.freq(s) != 0 {
                    break;
                }
            }
            esc_freq = esc_freq.wrapping_add(removed);
            let new_ns = num_stats - removed;
            self.a.wr8(mc, new_ns as u8);
            if new_ns == 0 {
                let mut tmp = self.get_state(stats);
                tmp.freq = ((2 * tmp.freq as u32 + esc_freq - 1) / esc_freq).min(MAX_FREQ / 3) as u8;
                let idx = self.a.u2i((num_stats + 2) >> 1);
                self.a.insert_node(stats, idx);
                let fl = (self.flags(mc) & 0x10) + 0x08 * (tmp.sym >= 0x40) as u8;
                self.a.wr8(mc + 1, fl);
                self.found_state = mc + 2;
                self.put_state(mc + 2, tmp);
                return;
            }
            let n0 = (num_stats + 2) >> 1;
            let n1 = (new_ns + 2) >> 1;
            if n0 != n1 {
                let ns2 = self.a.shrink_units(stats, n0, n1);
                self.a.wr32(mc + 4, ns2);
            }
            // Recompute the "some symbol >= 0x40" flag.
            let mut fl = self.flags(mc) & !0x08;
            let mut p = self.stats(mc);
            for _ in 0..=new_ns {
                fl |= 0x08 * (self.sym(p) >= 0x40) as u8;
                p += STATE;
            }
            self.a.wr8(mc + 1, fl);
        }
        self.set_summ(mc, sum_freq + esc_freq - (esc_freq >> 1));
        let fl = self.flags(mc) | 0x04;
        self.a.wr8(mc + 1, fl);
        self.found_state = self.stats(mc);
    }

    fn next_context(&mut self) {
        let c = self.succ(self.found_state);
        if self.order_fall == 0 && c >= self.a.units_start {
            self.min_context = c;
            self.max_context = c;
        } else {
            self.update_model();
            self.min_context = self.max_context;
        }
    }

    fn update1(&mut self) {
        let mut s = self.found_state;
        self.set_freq(s, self.freq(s) + 4);
        let sf = self.summ(self.min_context) + 4;
        self.set_summ(self.min_context, sf);
        if self.freq(s) > self.freq(s - STATE) {
            self.swap_states(s, s - STATE);
            s -= STATE;
            self.found_state = s;
            if self.freq(s) > MAX_FREQ {
                self.rescale();
            }
        }
        self.next_context();
    }

    fn update1_0(&mut self) {
        let fs = self.found_state;
        let mc = self.min_context;
        self.prev_success = (2 * self.freq(fs) >= self.summ(mc)) as u32;
        self.run_length += self.prev_success as i32;
        let sf = self.summ(mc) + 4;
        self.set_summ(mc, sf);
        let f = self.freq(fs) + 4;
        self.set_freq(fs, f);
        if f > MAX_FREQ {
            self.rescale();
        }
        self.next_context();
    }

    fn update2(&mut self) {
        let fs = self.found_state;
        let mc = self.min_context;
        let sf = self.summ(mc) + 4;
        self.set_summ(mc, sf);
        let f = self.freq(fs) + 4;
        self.set_freq(fs, f);
        if f > MAX_FREQ {
            self.rescale();
        }
        self.run_length = self.init_rl;
        self.update_model();
        self.min_context = self.max_context;
    }

    fn update_bin(&mut self) {
        let fs = self.found_state;
        let f = self.freq(fs);
        self.set_freq(fs, f + (f < 196) as u32);
        self.prev_success = 1;
        self.run_length += 1;
        self.next_context();
    }

    /// Choose the SEE (secondary escape estimation) slot for the current
    /// multi-symbol context and return it with the escape frequency.
    fn make_esc_freq(&mut self, num_masked: u32) -> (usize, u32) {
        let mc = self.min_context;
        let ns = self.ns(mc);
        if ns != 0xFF {
            let row = self.ns2indx[ns as usize + 2] as usize - 3;
            let col = (self.summ(mc) > 11 * (ns + 1)) as usize
                + 2 * (2 * ns < self.ns(self.suffix(mc)) + num_masked) as usize
                + self.flags(mc) as usize;
            let idx = row * 32 + col;
            let see = &mut self.see[idx];
            let r = (see.summ >> see.shift) as u32;
            see.summ = see.summ.wrapping_sub(r as u16);
            (idx, r + (r == 0) as u32)
        } else {
            (DUMMY_SEE, 1)
        }
    }

    #[inline]
    fn bin_index(&self) -> (usize, usize) {
        let mc = self.min_context;
        let f = self.freq(mc + 2);
        let row = self.ns2indx[f as usize - 1] as usize;
        let col = self.prev_success as usize
            + self.ns2bsindx[self.ns(self.suffix(mc)) as usize] as usize
            + self.flags(mc) as usize
            + ((self.run_length >> 26) & 0x20) as usize;
        (row, col)
    }

    // ---- decoding ----------------------------------------------------------

    /// Decode one byte.
    pub fn decode_symbol(&mut self, rc: &mut RangeDecoder) -> Result<u8, Error> {
        let mc = self.min_context;
        if self.ns(mc) != 0 {
            let mut s = self.stats(mc);
            let total = self.summ(mc);
            let count = rc.threshold(total)?;
            let mut hi = self.freq(s);
            if count < hi {
                rc.decode(0, hi);
                self.found_state = s;
                let sym = self.sym(s);
                self.update1_0();
                return Ok(sym);
            }
            self.prev_success = 0;
            let mut i = self.ns(mc);
            loop {
                s += STATE;
                let f = self.freq(s);
                hi += f;
                if hi > count {
                    rc.decode(hi - f, f);
                    self.found_state = s;
                    let sym = self.sym(s);
                    self.update1();
                    return Ok(sym);
                }
                i -= 1;
                if i == 0 {
                    break;
                }
            }
            if count >= total {
                return Err(Error::Corrupt("symbol count out of range"));
            }
            rc.decode(hi, total - hi);
            self.new_mask();
            let st = self.stats(mc) as usize;
            let len = (self.ns(mc) as usize + 1) * STATE as usize;
            if let Some(bytes) = self.a.mem.get(st..st + len) {
                for c in bytes.chunks_exact(STATE as usize) {
                    self.char_mask[c[0] as usize] = self.esc_count;
                }
            }
        } else {
            let (row, col) = self.bin_index();
            let prob = self.bin_summ[row][col] as u32;
            let count = rc.threshold(BIN_SCALE)?;
            if count < prob {
                rc.decode(0, prob);
                self.bin_summ[row][col] = (prob + (1 << INT_BITS) - get_mean(prob)) as u16;
                self.found_state = mc + 2;
                let sym = self.sym(mc + 2);
                self.update_bin();
                return Ok(sym);
            }
            if count >= BIN_SCALE {
                return Err(Error::Corrupt("binary count out of range"));
            }
            rc.decode(prob, BIN_SCALE - prob);
            let np = prob - get_mean(prob);
            self.bin_summ[row][col] = np as u16;
            self.init_esc = EXP_ESCAPE[(np >> 10) as usize] as u32;
            self.new_mask();
            let sy = self.sym(mc + 2);
            self.char_mask[sy as usize] = self.esc_count;
            self.prev_success = 0;
        }
        self.escape_loop(rc)
    }

    /// Start a new set of excluded symbols.
    fn new_mask(&mut self) {
        self.esc_count = self.esc_count.wrapping_add(1);
        if self.esc_count == 0 {
            self.char_mask = [0; 256];
            self.esc_count = 1;
        }
    }

    fn escape_loop(&mut self, rc: &mut RangeDecoder) -> Result<u8, Error> {
        loop {
            let num_masked = self.ns(self.min_context);
            loop {
                self.order_fall += 1;
                let suf = self.suffix(self.min_context);
                if suf == 0 {
                    return Err(Error::Corrupt("escape from order-0 context"));
                }
                self.min_context = suf;
                if self.ns(suf) != num_masked {
                    break;
                }
            }
            let mc = self.min_context;
            let mut hi = 0u32;
            let s = self.stats(mc);
            let mut i = 0usize;
            let num = (self.ns(mc) - num_masked) as usize;
            {
                // Collect the not-yet-excluded symbols of this context.
                let base = s as usize;
                let len = (self.ns(mc) as usize + 1) * STATE as usize;
                let Some(bytes) = self.a.mem.get(base..base + len) else {
                    return Err(Error::Corrupt("inconsistent model"));
                };
                let esc_count = self.esc_count;
                for (j, st) in bytes.chunks_exact(STATE as usize).enumerate() {
                    if self.char_mask[st[0] as usize] != esc_count {
                        hi += st[1] as u32;
                        self.ps[i] = s + (j as u32) * STATE;
                        self.ps_sym[i] = st[0];
                        self.ps_freq[i] = st[1];
                        i += 1;
                        if i == num {
                            break;
                        }
                    }
                }
                if i != num {
                    return Err(Error::Corrupt("inconsistent model"));
                }
            }
            let (see, esc) = self.make_esc_freq(num_masked);
            let total = esc + hi;
            let count = rc.threshold(total)?;
            if count < hi {
                let mut acc = 0u32;
                let mut k = 0usize;
                loop {
                    acc += self.ps_freq[k] as u32;
                    if acc > count {
                        break;
                    }
                    k += 1;
                }
                let s = self.ps[k];
                let f = self.ps_freq[k] as u32;
                rc.decode(acc - f, f);
                self.see[see].update();
                self.found_state = s;
                let sym = self.sym(s);
                self.update2();
                return Ok(sym);
            }
            if count >= total {
                return Err(Error::Corrupt("escape count out of range"));
            }
            rc.decode(hi, total - hi);
            self.see[see].summ = self.see[see].summ.wrapping_add(total as u16);
            for k in 0..i {
                self.char_mask[self.ps_sym[k] as usize] = self.esc_count;
            }
        }
    }
}

#[inline(always)]
fn get_mean(prob: u32) -> u32 {
    (prob + (1 << (PERIOD_BITS - 2))) >> PERIOD_BITS
}

#[allow(dead_code)]
const _: usize = NUM_INDEXES;

#[cfg(any(test, feature = "encoder"))]
mod encoder;
#[cfg(test)]
mod tests;
