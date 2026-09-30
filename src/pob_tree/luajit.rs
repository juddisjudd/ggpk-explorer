//! The order LuaJIT 2.1's `pairs` walks a table whose keys are small positive
//! integers assigned one at a time to a fresh `{}`.
//!
//! PoB-PoE2 builds each group's `orbits` list that way (`orbits[r + 1] = true`
//! per node, then `for orbit in pairs(orbits)`), so the list comes out in hash
//! order, not sorted. This follows `lj_tab.c`: the array part holds keys below
//! its size, the rest go in the hash part by `hashnum`, a full hash part is
//! rehashed with `bestasize` choosing a new array size, and `pairs` visits the
//! array part and then the hash nodes in slot order.

const MAX_ABITS: usize = 28;

#[derive(Clone, Copy, Default)]
struct Node {
    key: Option<i32>,
    next: Option<usize>,
}

#[derive(Default)]
struct Tab {
    array: Vec<bool>,
    nodes: Vec<Node>,
    hmask: u32,
    freetop: usize,
}

fn hashrot(mut lo: u32, mut hi: u32) -> u32 {
    lo ^= hi;
    hi = hi.rotate_left(14);
    lo = lo.wrapping_sub(hi);
    hi = hi.rotate_left(5);
    hi ^= lo;
    hi.wrapping_sub(lo.rotate_left(13))
}

fn fls(x: u32) -> u32 {
    31 - x.leading_zeros()
}

fn hsize2hbits(s: u32) -> u32 {
    match s {
        0 => 0,
        1 => 1,
        s => 1 + fls(s - 1),
    }
}

fn countint(key: i32, bins: &mut [u32; MAX_ABITS]) -> u32 {
    if key < 0 {
        return 0;
    }
    let k = key as u32;
    bins[if k > 2 { fls(k - 1) as usize } else { 0 }] += 1;
    1
}

fn bestasize(bins: &[u32; MAX_ABITS], narray: &mut u32) -> u32 {
    let (mut sum, mut na, mut sz) = (0u32, 0u32, 0u32);
    let nn = *narray;
    let mut b = 0;
    while b < MAX_ABITS && 2 * nn > (1u32 << b) && sum != nn {
        if bins[b] > 0 {
            sum += bins[b];
            if 2 * sum > (1u32 << b) {
                sz = (2u32 << b) + 1;
                na = sum;
            }
        }
        b += 1;
    }
    *narray = sz;
    na
}

impl Tab {
    fn main_position(&self, key: i32) -> usize {
        let bits = (key as f64).to_bits();
        (hashrot(bits as u32, ((bits >> 32) as u32) << 1) & self.hmask) as usize
    }

    fn has_hash(&self) -> bool {
        self.hmask > 0
    }

    fn set(&mut self, key: i32) {
        if key >= 0 && (key as usize) < self.array.len() {
            self.array[key as usize] = true;
            return;
        }
        self.set_hash(key);
    }

    fn set_hash(&mut self, key: i32) {
        if self.has_hash() {
            let mut n = Some(self.main_position(key));
            while let Some(i) = n {
                if self.nodes[i].key == Some(key) {
                    return;
                }
                n = self.nodes[i].next;
            }
        }
        self.new_key(key);
    }

    fn new_key(&mut self, key: i32) {
        if !self.has_hash() {
            self.rehash(key);
            return self.set(key);
        }
        let mut n = self.main_position(key);
        if self.nodes[n].key.is_some() {
            let mut free = self.freetop;
            loop {
                if free == 0 {
                    self.rehash(key);
                    return self.set(key);
                }
                free -= 1;
                if self.nodes[free].key.is_none() {
                    break;
                }
            }
            self.freetop = free;
            let collide = self.main_position(self.nodes[n].key.expect("occupied node has a key"));
            if collide != n {
                let mut c = collide;
                while self.nodes[c].next != Some(n) {
                    c = self.nodes[c].next.expect("colliding chain reaches the main node");
                }
                self.nodes[c].next = Some(free);
                self.nodes[free] = self.nodes[n];
                self.nodes[n] = Node::default();
                let mut f = free;
                while let Some(nn) = self.nodes[f].next {
                    match self.nodes[nn].key {
                        Some(k) if self.main_position(k) == n => {
                            self.nodes[f].next = self.nodes[nn].next;
                            self.nodes[nn].next = self.nodes[n].next;
                            self.nodes[n].next = Some(nn);
                            break;
                        }
                        _ => f = nn,
                    }
                }
            } else {
                self.nodes[free].next = self.nodes[n].next;
                self.nodes[n].next = Some(free);
                n = free;
            }
        }
        self.nodes[n].key = Some(key);
    }

    fn rehash(&mut self, extra: i32) {
        let mut bins = [0u32; MAX_ABITS];
        let mut na = self.count_array(&mut bins);
        let mut total = 1 + na;
        for node in &self.nodes {
            if let Some(k) = node.key {
                na += countint(k, &mut bins);
                total += 1;
            }
        }
        na += countint(extra, &mut bins);
        let in_array = bestasize(&bins, &mut na);
        total -= in_array;
        self.resize(na as usize, hsize2hbits(total));
    }

    fn count_array(&self, bins: &mut [u32; MAX_ABITS]) -> u32 {
        let asize = self.array.len();
        if asize == 0 {
            return 0;
        }
        let (mut na, mut i) = (0u32, 0usize);
        for (b, bin) in bins.iter_mut().enumerate() {
            let mut top = 2usize << b;
            if top >= asize {
                top = asize - 1;
                if i > top {
                    break;
                }
            }
            let mut n = 0;
            while i <= top {
                if self.array[i] {
                    n += 1;
                }
                i += 1;
            }
            *bin += n;
            na += n;
        }
        na
    }

    fn resize(&mut self, asize: usize, hbits: u32) {
        let old_nodes = std::mem::take(&mut self.nodes);
        let old_has_hash = self.has_hash();
        let old_asize = self.array.len();
        if asize > old_asize {
            self.array.resize(asize, false);
        }
        if hbits > 0 {
            let hsize = 1usize << hbits;
            self.nodes = vec![Node::default(); hsize];
            self.hmask = (hsize - 1) as u32;
            self.freetop = hsize;
        } else {
            self.hmask = 0;
            self.freetop = 0;
        }
        if asize < old_asize {
            let moved: Vec<i32> = (asize..old_asize).filter(|&i| self.array[i]).map(|i| i as i32).collect();
            self.array.truncate(asize);
            for key in moved {
                self.set_hash(key);
            }
        }
        if old_has_hash {
            for node in old_nodes {
                if let Some(key) = node.key {
                    self.set(key);
                }
            }
        }
    }

    fn pairs(&self) -> Vec<i32> {
        let array = self.array.iter().enumerate().filter(|(_, set)| **set).map(|(i, _)| i as i32);
        let hash = self.nodes.iter().filter_map(|n| n.key);
        array.chain(hash).collect()
    }
}

/// Keys in the order `pairs` returns them after `t[k] = true` for each of
/// `keys` in turn on an empty table. A repeated key changes nothing.
pub fn pairs_order(keys: impl IntoIterator<Item = i32>) -> Vec<i32> {
    let mut table = Tab::default();
    for key in keys {
        table.set(key);
    }
    table.pairs()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Printed by `luajit` 2.1: assign `t[k] = true` for each key, then list
    /// `pairs(t)`.
    const CASES: [(&[i32], &[i32]); 14] = [
        (&[3, 4], &[4, 3]),
        (&[5, 1], &[5, 1]),
        (&[8, 3], &[3, 8]),
        (&[7, 10, 9], &[7, 10, 9]),
        (&[10, 10, 2, 7], &[2, 10, 7]),
        (&[2, 6, 1, 2, 2, 7, 7], &[1, 2, 7, 6]),
        (&[6, 8, 1, 9, 10, 10, 9, 10, 2], &[1, 2, 6, 8, 10, 9]),
        (&[7, 3, 9], &[7, 9, 3]),
        (&[5, 6, 10], &[10, 5, 6]),
        (&[4, 9], &[9, 4]),
        (&[10, 2], &[10, 2]),
        (&[2, 9, 9, 9, 5, 8, 5, 10, 9, 1], &[1, 2, 5, 8, 10, 9]),
        (&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
        (&[10, 9, 8, 7, 6, 5, 4, 3, 2, 1], &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
    ];

    #[test]
    fn matches_luajit_pairs() {
        for (keys, expected) in CASES {
            assert_eq!(pairs_order(keys.iter().copied()), expected, "keys {:?}", keys);
        }
    }

    /// Checks random insertion orders against a `luajit` on PATH.
    #[test]
    #[ignore]
    fn matches_luajit_on_random_keys() {
        let script = "math.randomseed(7) for _ = 1, 2000 do local n, seq, t, out = math.random(1, 10), {}, {}, {} \
            for j = 1, n do seq[j] = math.random(1, 10) t[seq[j]] = true end \
            for k in pairs(t) do out[#out + 1] = k end print(table.concat(seq, ',') .. ';' .. table.concat(out, ',')) end";
        let output = std::process::Command::new("luajit").args(["-e", script]).output().expect("luajit on PATH");
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let (keys, expected) = line.split_once(';').expect("keys;order");
            let parse = |s: &str| s.split(',').map(|k| k.parse::<i32>().expect("integer")).collect::<Vec<_>>();
            assert_eq!(pairs_order(parse(keys)), parse(expected), "keys {}", keys);
        }
    }
}
