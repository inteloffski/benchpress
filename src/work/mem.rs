//! Memory subsystem: streaming bandwidth (emulators, large heaps, linking big binaries)
//! and dependent-load latency (pointer-heavy compiler/VM data structures).

use crate::util::{mix, Rng};

const BW_ELEMS: usize = 16 << 20; // 3 arrays x 128 MiB
const BW_PASSES: usize = 400;
const LAT_ELEMS: usize = 32 << 20; // 128 MiB of u32
const LAT_STEPS: usize = 12_000_000;

fn triad(a: &mut [u64], b: &[u64], c: &[u64], k: u64, threads: usize) {
    let per = (a.len() + threads - 1) / threads;
    std::thread::scope(|s| {
        for ((ac, bc), cc) in a.chunks_mut(per).zip(b.chunks(per)).zip(c.chunks(per)) {
            s.spawn(move || {
                for i in 0..ac.len() {
                    ac[i] = bc[i].wrapping_add(k.wrapping_mul(cc[i]));
                }
            });
        }
    });
}

pub fn setup_bw() -> Box<dyn FnMut(usize) -> u64> {
    let mut a = vec![1u64; BW_ELEMS];
    let b: Vec<u64> = (0..BW_ELEMS as u64).collect();
    let c: Vec<u64> = (0..BW_ELEMS as u64).map(|x| x ^ 0x5555).collect();
    Box::new(move |threads| {
        for p in 0..BW_PASSES {
            if p % 2 == 0 {
                triad(&mut a, &b, &c, 3, threads);
            } else {
                triad(&mut a, &c, &b, 5, threads);
            }
        }
        [0, BW_ELEMS / 3, BW_ELEMS / 2, BW_ELEMS - 1].iter().fold(0, |h, &i| mix(h, a[i]))
    })
}

pub fn setup_lat() -> Box<dyn FnMut(usize) -> u64> {
    // Sattolo's algorithm: a single random cycle through the whole buffer
    let mut next: Vec<u32> = (0..LAT_ELEMS as u32).collect();
    let mut r = Rng::new(3);
    for i in (1..LAT_ELEMS).rev() {
        let j = r.below(i);
        next.swap(i, j);
    }
    Box::new(move |_| {
        let mut p = 0u32;
        for _ in 0..LAT_STEPS {
            p = next[p as usize];
        }
        p as u64
    })
}
