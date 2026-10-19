//! Sub-allocator of the PPMd (variant I) model.
//!
//! All model data lives in one flat byte arena addressed by 32-bit offsets
//! (offset 0 is "null").  The arena is split in two parts that grow towards
//! each other:
//!
//! * the *text area* at the bottom, where raw input bytes are stored while
//!   contexts are not yet created for them, and
//! * the *units area* above `units_start`, made of 12-byte units.  Contexts
//!   take one unit; symbol-statistics arrays (6 bytes per symbol) take
//!   several.  `lo_unit..hi_unit` is the never-yet-used gap: statistics are
//!   carved from `lo_unit` upwards, contexts from `hi_unit` downwards.
//!
//! Freed blocks go to per-size free lists (38 size classes, see
//! `indx2units`).  Because memory exhaustion triggers a model restart, the
//! exact moment at which allocation fails must match the encoder, so the
//! block-splitting and free-block-gluing behaviour here follows the reference
//! algorithm precisely.

pub const UNIT_SIZE: u32 = 12;
pub const NUM_INDEXES: usize = 38;
const EMPTY_NODE: u32 = 0xFFFF_FFFF;

pub struct Arena {
    pub mem: Vec<u8>,
    size: u32,
    align_offset: u32,
    pub text: u32,
    pub units_start: u32,
    pub lo_unit: u32,
    pub hi_unit: u32,
    glue_count: u32,
    free_list: [u32; NUM_INDEXES],
    indx2units: [u8; NUM_INDEXES],
    units2indx: [u8; 128],
}

impl Arena {
    /// `size` is the model memory in bytes (caller validates the range).
    pub fn new(size: u32) -> Arena {
        let align_offset = 4 - (size & 3);
        let mut indx2units = [0u8; NUM_INDEXES];
        let mut units2indx = [0u8; 128];
        // Size classes: 1,2,3,4 units, then steps of 2, 3 and 4 units up to 128.
        let mut k = 0usize;
        for i in 0..NUM_INDEXES {
            let step = if i >= 12 { 4 } else { (i >> 2) + 1 };
            for _ in 0..step {
                units2indx[k] = i as u8;
                k += 1;
            }
            indx2units[i] = k as u8;
        }
        let mem = vec![0u8; (align_offset + size + UNIT_SIZE) as usize];
        let mut a = Arena {
            mem,
            size,
            align_offset,
            text: 0,
            units_start: 0,
            lo_unit: 0,
            hi_unit: 0,
            glue_count: 0,
            free_list: [0; NUM_INDEXES],
            indx2units,
            units2indx,
        };
        a.reset();
        a
    }

    /// Forget everything: empty free lists, empty text area, 1/8 of the
    /// memory is reserved for text and 7/8 for units.
    pub fn reset(&mut self) {
        self.free_list = [0; NUM_INDEXES];
        self.text = self.align_offset;
        self.hi_unit = self.text + self.size;
        self.units_start = self.hi_unit - self.size / 8 / UNIT_SIZE * 7 * UNIT_SIZE;
        self.lo_unit = self.units_start;
        self.glue_count = 0;
    }

    // ---- raw little-endian accessors -------------------------------------

    #[inline(always)]
    pub fn rd8(&self, o: u32) -> u8 {
        self.mem[o as usize]
    }
    #[inline(always)]
    pub fn wr8(&mut self, o: u32, v: u8) {
        self.mem[o as usize] = v;
    }
    #[inline(always)]
    pub fn rd16(&self, o: u32) -> u16 {
        let o = o as usize;
        u16::from_le_bytes(self.mem[o..o + 2].try_into().unwrap())
    }
    #[inline(always)]
    pub fn wr16(&mut self, o: u32, v: u16) {
        let o = o as usize;
        self.mem[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    #[inline(always)]
    pub fn rd32(&self, o: u32) -> u32 {
        let o = o as usize;
        u32::from_le_bytes(self.mem[o..o + 4].try_into().unwrap())
    }
    #[inline(always)]
    pub fn wr32(&mut self, o: u32, v: u32) {
        let o = o as usize;
        self.mem[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    #[inline(always)]
    pub fn i2u(&self, indx: usize) -> u32 {
        self.indx2units[indx] as u32
    }
    /// Size class for a block of `nu` (1..=128) units.
    #[inline(always)]
    pub fn u2i(&self, nu: u32) -> usize {
        self.units2indx[nu as usize - 1] as usize
    }

    // ---- free lists --------------------------------------------------------
    // A free block starts with: stamp (u32, 0xFFFFFFFF), next (u32), nu (u32).

    pub fn insert_node(&mut self, node: u32, indx: usize) {
        self.wr32(node, EMPTY_NODE);
        self.wr32(node + 4, self.free_list[indx]);
        let nu = self.i2u(indx);
        self.wr32(node + 8, nu);
        self.free_list[indx] = node;
    }

    fn remove_node(&mut self, indx: usize) -> u32 {
        let node = self.free_list[indx];
        self.free_list[indx] = self.rd32(node + 4);
        node
    }

    /// Return the tail of a block of size class `old_indx` to the free lists,
    /// keeping only the first `new_indx`-sized part.
    fn split_block(&mut self, ptr: u32, old_indx: usize, new_indx: usize) {
        let nu = self.i2u(old_indx) - self.i2u(new_indx);
        let ptr = ptr + self.i2u(new_indx) * UNIT_SIZE;
        let mut i = self.u2i(nu);
        if self.i2u(i) != nu {
            i -= 1;
            let k = self.i2u(i);
            self.insert_node(ptr + k * UNIT_SIZE, (nu - k - 1) as usize);
        }
        self.insert_node(ptr, i);
    }

    /// Merge physically adjacent free blocks and redistribute them over the
    /// free lists.
    fn glue_free_blocks(&mut self) {
        self.glue_count = 1 << 13;
        // Guard so that gluing stops at the start of the unused gap.
        if self.lo_unit != self.hi_unit {
            self.wr32(self.lo_unit, 0);
        }
        // Chain all non-empty free blocks (in list order) while absorbing
        // the free blocks that directly follow them in memory.
        let mut head = 0u32;
        let mut prev = 0u32;
        for i in 0..NUM_INDEXES {
            let mut next = self.free_list[i];
            self.free_list[i] = 0;
            while next != 0 {
                let node = next;
                if self.rd32(node + 8) != 0 {
                    if prev == 0 {
                        head = node;
                    } else {
                        self.wr32(prev + 4, node);
                    }
                    prev = node;
                    loop {
                        let nu = self.rd32(node + 8);
                        let node2 = node + nu * UNIT_SIZE;
                        if self.rd32(node2) != EMPTY_NODE {
                            break;
                        }
                        let nu2 = self.rd32(node2 + 8);
                        self.wr32(node + 8, nu + nu2);
                        self.wr32(node2 + 8, 0);
                    }
                }
                next = self.rd32(node + 4);
            }
        }
        if prev != 0 {
            self.wr32(prev + 4, 0);
        }
        // Refill the free lists from the glued blocks.
        let mut next = head;
        while next != 0 {
            let mut node = next;
            next = self.rd32(node + 4);
            let mut nu = self.rd32(node + 8);
            if nu == 0 {
                continue;
            }
            while nu > 128 {
                self.insert_node(node, NUM_INDEXES - 1);
                nu -= 128;
                node += 128 * UNIT_SIZE;
            }
            let mut i = self.u2i(nu);
            if self.i2u(i) != nu {
                i -= 1;
                let k = self.i2u(i);
                self.insert_node(node + k * UNIT_SIZE, (nu - k - 1) as usize);
            }
            self.insert_node(node, i);
        }
    }

    /// Slow allocation path.  Returns 0 when memory is exhausted.
    fn alloc_units_rare(&mut self, indx: usize) -> u32 {
        if self.glue_count == 0 {
            self.glue_free_blocks();
            if self.free_list[indx] != 0 {
                return self.remove_node(indx);
            }
        }
        let mut i = indx;
        loop {
            i += 1;
            if i == NUM_INDEXES {
                // Take the space from the top of the text area.
                let num_bytes = self.i2u(indx) * UNIT_SIZE;
                self.glue_count = self.glue_count.wrapping_sub(1);
                return if self.units_start - self.text > num_bytes {
                    self.units_start -= num_bytes;
                    self.units_start
                } else {
                    0
                };
            }
            if self.free_list[i] != 0 {
                break;
            }
        }
        let block = self.remove_node(i);
        self.split_block(block, i, indx);
        block
    }

    /// Allocate a block of size class `indx`; 0 on exhaustion.
    pub fn alloc_units(&mut self, indx: usize) -> u32 {
        if self.free_list[indx] != 0 {
            return self.remove_node(indx);
        }
        let num_bytes = self.i2u(indx) * UNIT_SIZE;
        if num_bytes <= self.hi_unit - self.lo_unit {
            let r = self.lo_unit;
            self.lo_unit += num_bytes;
            return r;
        }
        self.alloc_units_rare(indx)
    }

    /// Allocate a single unit for a context; 0 on exhaustion.
    pub fn alloc_context(&mut self) -> u32 {
        if self.hi_unit != self.lo_unit {
            self.hi_unit -= UNIT_SIZE;
            self.hi_unit
        } else if self.free_list[0] != 0 {
            self.remove_node(0)
        } else {
            self.alloc_units_rare(0)
        }
    }

    /// Shrink a block from `old_nu` to `new_nu` units, possibly moving it.
    pub fn shrink_units(&mut self, old_ptr: u32, old_nu: u32, new_nu: u32) -> u32 {
        let i0 = self.u2i(old_nu);
        let i1 = self.u2i(new_nu);
        if i0 == i1 {
            return old_ptr;
        }
        if self.free_list[i1] != 0 {
            let ptr = self.remove_node(i1);
            let n = (new_nu * UNIT_SIZE) as usize;
            self.mem.copy_within(old_ptr as usize..old_ptr as usize + n, ptr as usize);
            self.insert_node(old_ptr, i0);
            return ptr;
        }
        self.split_block(old_ptr, i0, i1);
        old_ptr
    }

    /// Move a statistics block to a larger size class (used when growing).
    pub fn copy_units(&mut self, dst: u32, src: u32, nu: u32) {
        let n = (nu * UNIT_SIZE) as usize;
        self.mem.copy_within(src as usize..src as usize + n, dst as usize);
    }
}
