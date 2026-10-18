//! Packaging model (APK / AAB / IPA zipping, asset compression): LZ77 with hash chains —
//! the match finder that dominates deflate — plus a verifying decompress.

use crate::util::{fnv1a, mix, par_jobs, Rng};
use crate::work::compile::gen_source;

const BLOCK: usize = 256 * 1024;
const BLOCKS: usize = 64;
const JOBS: usize = 4096;

const MIN_MATCH: usize = 4;
const MAX_MATCH: usize = 258;
const WINDOW: usize = 32 * 1024;
const HASH_BITS: u32 = 15;
const MAX_CHAIN: usize = 48;

pub fn setup() -> Box<dyn FnMut(usize) -> u64> {
    // corpus: half source code, half structured binary (dex/class/asset-like records)
    let mut corpus = Vec::with_capacity(BLOCK * BLOCKS);
    let mut seed = 0;
    while corpus.len() < BLOCK * BLOCKS / 2 {
        corpus.extend_from_slice(gen_source(500 + seed, 200).as_bytes());
        seed += 1;
    }
    corpus.truncate(BLOCK * BLOCKS / 2);
    let mut r = Rng::new(7);
    while corpus.len() < BLOCK * BLOCKS {
        let kind = r.below(4) as u8;
        corpus.extend_from_slice(&[0xCA, 0xFE, kind, 0]);
        let id = r.below(4096) as u32;
        corpus.extend_from_slice(&id.to_le_bytes());
        let len = r.range(8, 64);
        if r.chance(25) {
            let mut noise = vec![0u8; len];
            r.fill(&mut noise);
            corpus.extend_from_slice(&noise);
        } else {
            for j in 0..len {
                corpus.push((id as usize * 31 + j * kind as usize) as u8);
            }
        }
    }
    corpus.truncate(BLOCK * BLOCKS);
    Box::new(move |threads| {
        par_jobs(threads, JOBS, |i| {
            let b = i % BLOCKS;
            let src = &corpus[b * BLOCK..(b + 1) * BLOCK];
            let mut packed = Vec::with_capacity(BLOCK / 2);
            compress(src, &mut packed);
            let mut unpacked = Vec::with_capacity(BLOCK);
            decompress(&packed, &mut unpacked);
            assert!(unpacked == src, "LZ roundtrip failed");
            mix(b as u64, mix(packed.len() as u64, fnv1a(&packed[..packed.len().min(4096)])))
        })
    })
}

fn varint(out: &mut Vec<u8>, mut v: usize) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn read_varint(src: &[u8], pos: &mut usize) -> usize {
    let mut v = 0usize;
    let mut shift = 0;
    loop {
        let b = src[*pos];
        *pos += 1;
        v |= ((b & 0x7f) as usize) << shift;
        if b < 0x80 {
            return v;
        }
        shift += 7;
    }
}

#[inline]
fn hash4(b: &[u8]) -> usize {
    let v = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize
}

/// Format: repeated [lit_len][literals][match_len][offset?], match_len == 0 terminates.
pub fn compress(src: &[u8], out: &mut Vec<u8>) {
    let mut head = vec![u32::MAX; 1 << HASH_BITS];
    let mut prev = vec![u32::MAX; src.len()];
    let mut i = 0;
    let mut lit_start = 0;
    while i + MIN_MATCH <= src.len() {
        let h = hash4(&src[i..]);
        let mut cand = head[h];
        let (mut best_len, mut best_off) = (0, 0);
        let max = (src.len() - i).min(MAX_MATCH);
        let mut chain = MAX_CHAIN;
        while cand != u32::MAX && chain > 0 {
            let c = cand as usize;
            if i - c > WINDOW {
                break;
            }
            if src[c + best_len.min(max - 1)] == src[i + best_len.min(max - 1)] {
                let mut l = 0;
                while l < max && src[c + l] == src[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_off = i - c;
                    if l == max {
                        break;
                    }
                }
            }
            cand = prev[c];
            chain -= 1;
        }
        prev[i] = head[h];
        head[h] = i as u32;
        if best_len >= MIN_MATCH {
            varint(out, i - lit_start);
            out.extend_from_slice(&src[lit_start..i]);
            varint(out, best_len);
            varint(out, best_off);
            for j in i + 1..i + best_len {
                if j + MIN_MATCH <= src.len() {
                    let h = hash4(&src[j..]);
                    prev[j] = head[h];
                    head[h] = j as u32;
                }
            }
            i += best_len;
            lit_start = i;
        } else {
            i += 1;
        }
    }
    varint(out, src.len() - lit_start);
    out.extend_from_slice(&src[lit_start..]);
    varint(out, 0);
}

pub fn decompress(src: &[u8], out: &mut Vec<u8>) {
    let mut pos = 0;
    loop {
        let lits = read_varint(src, &mut pos);
        out.extend_from_slice(&src[pos..pos + lits]);
        pos += lits;
        let len = read_varint(src, &mut pos);
        if len == 0 {
            return;
        }
        let off = read_varint(src, &mut pos);
        let start = out.len() - off;
        for k in 0..len {
            let b = out[start + k];
            out.push(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut r = Rng::new(1);
        let mut data = gen_source(3, 50).into_bytes();
        let mut noise = vec![0u8; 5000];
        r.fill(&mut noise);
        data.extend_from_slice(&noise);
        data.extend(std::iter::repeat(b'z').take(3000));
        let mut packed = Vec::new();
        compress(&data, &mut packed);
        assert!(packed.len() < data.len());
        let mut out = Vec::new();
        decompress(&packed, &mut out);
        assert!(out == data);
    }
}
