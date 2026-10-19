//! Test/example-only PPMd var.I encoder mirroring `decode_symbol` (compiled
//! with `cfg(test)` or the `encoder` feature; never part of the default build).

use super::*;
use crate::ppmd::rangecoder::RangeEncoder;

impl Model {
    pub(crate) fn encode_symbol(&mut self, rc: &mut RangeEncoder, sym: u8) {
        let mut mask = [0xFFu8; 256];
        let mc = self.min_context;
        if self.ns(mc) != 0 {
            let mut s = self.stats(mc);
            let total = self.summ(mc);
            rc.scale(total);
            if self.sym(s) == sym {
                rc.encode(0, self.freq(s));
                self.found_state = s;
                self.update1_0();
                return;
            }
            self.prev_success = 0;
            let mut lo = self.freq(s);
            for _ in 0..self.ns(mc) {
                s += STATE;
                if self.sym(s) == sym {
                    rc.encode(lo, self.freq(s));
                    self.found_state = s;
                    self.update1();
                    return;
                }
                lo += self.freq(s);
            }
            rc.encode(lo, total - lo);
            let st = self.stats(mc);
            for j in 0..=self.ns(mc) {
                mask[self.sym(st + j * STATE) as usize] = 0;
            }
        } else {
            let (row, col) = self.bin_index();
            let prob = self.bin_summ[row][col] as u32;
            rc.scale(BIN_SCALE);
            if self.sym(mc + 2) == sym {
                rc.encode(0, prob);
                self.bin_summ[row][col] = (prob + (1 << INT_BITS) - get_mean(prob)) as u16;
                self.found_state = mc + 2;
                self.update_bin();
                return;
            }
            rc.encode(prob, BIN_SCALE - prob);
            let np = prob - get_mean(prob);
            self.bin_summ[row][col] = np as u16;
            self.init_esc = EXP_ESCAPE[(np >> 10) as usize] as u32;
            mask[self.sym(mc + 2) as usize] = 0;
            self.prev_success = 0;
        }
        loop {
            let num_masked = self.ns(self.min_context);
            loop {
                self.order_fall += 1;
                let suf = self.suffix(self.min_context);
                assert!(suf != 0);
                self.min_context = suf;
                if self.ns(suf) != num_masked {
                    break;
                }
            }
            let mc = self.min_context;
            let st = self.stats(mc);
            let mut ps = Vec::new();
            let mut hi = 0;
            for j in 0..=self.ns(mc) {
                let s = st + j * STATE;
                if mask[self.sym(s) as usize] != 0 {
                    hi += self.freq(s);
                    ps.push(s);
                }
            }
            let (see, esc) = self.make_esc_freq(num_masked);
            let total = esc + hi;
            rc.scale(total);
            if let Some(k) = ps.iter().position(|&s| self.sym(s) == sym) {
                let lo: u32 = ps[..k].iter().map(|&s| self.freq(s)).sum();
                let s = ps[k];
                rc.encode(lo, self.freq(s));
                self.see[see].update();
                self.found_state = s;
                self.update2();
                return;
            }
            rc.encode(hi, total - hi);
            self.see[see].summ = self.see[see].summ.wrapping_add(total as u16);
            for &p in &ps {
                mask[self.sym(p) as usize] = 0;
            }
        }
    }
}
