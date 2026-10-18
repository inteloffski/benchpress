//! Build cache keys / code signing / artifact checksums: SHA-256 over many 1 MiB chunks.

use crate::util::{fnv1a, mix, par_jobs, sha256, Rng};

const CHUNK: usize = 1 << 20;
const CHUNKS: usize = 32;
const JOBS: usize = 4800;

pub fn setup() -> Box<dyn FnMut(usize) -> u64> {
    let mut data = vec![0u8; CHUNK * CHUNKS];
    Rng::new(99).fill(&mut data);
    Box::new(move |threads| {
        par_jobs(threads, JOBS, |i| {
            let c = i % CHUNKS;
            let digest = sha256(&data[c * CHUNK..(c + 1) * CHUNK]);
            mix(c as u64, fnv1a(&digest))
        })
    })
}
