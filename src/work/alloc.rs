//! Managed-runtime model (Gradle daemon / Kotlin daemon / Dart VM / analysis server):
//! huge numbers of short-lived small objects, pointer-linked graphs and string-keyed maps.

use crate::util::{mix, par_jobs, Rng};
use std::collections::HashMap;

const JOBS: usize = 640;

struct Node {
    l: Option<Box<Node>>,
    r: Option<Box<Node>>,
    v: u32,
}

fn make(depth: u32, v: u32) -> Box<Node> {
    if depth == 0 {
        Box::new(Node { l: None, r: None, v })
    } else {
        Box::new(Node { l: Some(make(depth - 1, v.wrapping_mul(2))), r: Some(make(depth - 1, v.wrapping_mul(2) + 1)), v })
    }
}

fn check(n: &Node) -> u64 {
    let mut s = n.v as u64;
    if let Some(l) = &n.l {
        s = s.wrapping_add(check(l));
    }
    if let Some(r) = &n.r {
        s = s.wrapping_add(check(r)).rotate_left(1);
    }
    s
}

fn job(i: usize) -> u64 {
    let mut r = Rng::new(i as u64);
    let mut h = 0u64;

    // object graphs: many trees of varying depth (young-generation churn)
    let long_lived = make(14, i as u32);
    for depth in (4..=14).step_by(2) {
        let iters = 1usize << (14 - depth + 2);
        for k in 0..iters {
            let t = make(depth, k as u32);
            h = h.wrapping_add(check(&t));
        }
    }
    h = mix(h, check(&long_lived));

    // string-keyed maps: dependency graph / resolver / symbol index
    let mut map: HashMap<String, Vec<u32>> = HashMap::new();
    for k in 0..12_000u32 {
        let key = format!("com.example.feature{}.ui.Screen{}ViewModel", r.below(64), r.below(400));
        map.entry(key).or_default().push(k);
    }
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_unstable();
    for (n, k) in keys.iter().enumerate().step_by(3) {
        h = mix(h, (map[*k].len() as u64) << 20 | n as u64);
    }
    let removed: Vec<String> = keys.iter().step_by(2).map(|k| (*k).clone()).collect();
    for k in &removed {
        map.remove(k);
    }
    mix(h, map.len() as u64)
}

pub fn setup() -> Box<dyn FnMut(usize) -> u64> {
    Box::new(|threads| par_jobs(threads, JOBS, job))
}
