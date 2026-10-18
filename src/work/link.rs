//! Linker model (ld64 / lld / R8 symbol phase): build a global symbol table from many
//! object files, resolve undefined references, sort the output symbol table, lay out
//! addresses and apply relocations. Single-threaded, hash-table and cache-miss heavy.

use crate::util::{fnv1a, mix, FastMap, Rng};

const OBJECTS: usize = 800;
const DEFS_PER_OBJ: usize = 1000;
const REFS_PER_OBJ: usize = 2400;
const IMPORT_PCT: usize = 3;

const MODULES: &[&str] = &["Core", "Network", "Feed", "Profile", "Auth", "Analytics", "DesignSystem", "Payments", "Chat", "Media"];
const TYPES: &[&str] = &["ViewModel", "Repository", "Service", "Coordinator", "Cell", "Controller", "Store", "Mapper", "Factory", "Interactor"];
const METHODS: &[&str] = &["load", "reload", "update", "configure", "bind", "fetch", "decode", "render", "handle", "observe", "invalidate"];

struct Obj {
    strtab: Vec<u8>,
    defs: Vec<(u32, u32)>,
    refs: Vec<(u32, u32)>,
}

fn mangle(r: &mut Rng, id: usize) -> String {
    let m = MODULES[r.below(MODULES.len())];
    let t = TYPES[r.below(TYPES.len())];
    let f = METHODS[r.below(METHODS.len())];
    let item = format!("Item{}", id % 97);
    let ty = format!("{}{}{}", m, t, id / 7);
    let func = format!("{}{}{}", f, item, id);
    format!("_$s{}{}{}{}C{}{}yy{}SgF", m.len(), m, ty.len(), ty, func.len(), func, id % 13)
}

pub fn setup() -> Box<dyn FnMut(usize) -> u64> {
    let mut r = Rng::new(42);
    let total = OBJECTS * DEFS_PER_OBJ;
    let names: Vec<String> = (0..total).map(|i| mangle(&mut r, i)).collect();
    let objs: Vec<Obj> = (0..OBJECTS)
        .map(|o| {
            let mut obj = Obj { strtab: Vec::new(), defs: Vec::new(), refs: Vec::new() };
            let add = |obj: &mut Obj, s: &[u8], def: bool| {
                let off = obj.strtab.len() as u32;
                obj.strtab.extend_from_slice(s);
                obj.strtab.push(0);
                let e = (off, s.len() as u32);
                if def { obj.defs.push(e) } else { obj.refs.push(e) }
            };
            for d in 0..DEFS_PER_OBJ {
                add(&mut obj, names[o * DEFS_PER_OBJ + d].as_bytes(), true);
            }
            for _ in 0..REFS_PER_OBJ {
                if r.chance(IMPORT_PCT) {
                    let s = format!("_objc_msgSend$import{}", r.below(5000));
                    add(&mut obj, s.as_bytes(), false);
                } else {
                    // locality: most references point into nearby objects, like real modules
                    let target = if r.chance(70) {
                        (o + r.below(8)).min(OBJECTS - 1) * DEFS_PER_OBJ + r.below(DEFS_PER_OBJ)
                    } else {
                        r.below(total)
                    };
                    add(&mut obj, names[target].as_bytes(), false);
                }
            }
            obj
        })
        .collect();
    drop(names);
    Box::new(move |_| (0..3u64).fold(0, |h, k| mix(h, link(&objs) ^ k)))
}

fn link(objs: &[Obj]) -> u64 {
    // 1. global symbol table
    let mut table: FastMap<&[u8], u32> = FastMap::default();
    let mut all: Vec<&[u8]> = Vec::new();
    let mut dups = 0u64;
    for obj in objs {
        for &(off, len) in &obj.defs {
            let name = &obj.strtab[off as usize..(off + len) as usize];
            let id = all.len() as u32;
            if table.insert(name, id).is_some() {
                dups += 1;
            }
            all.push(name);
        }
    }

    // 2. sorted output symbol table + address layout
    let mut order: Vec<u32> = (0..all.len() as u32).collect();
    order.sort_unstable_by(|&a, &b| all[a as usize].cmp(all[b as usize]));
    let mut addr = vec![0u64; all.len()];
    let mut cursor = 0x1_0000_0000u64;
    for &i in &order {
        addr[i as usize] = cursor;
        cursor += 16 + (fnv1a(all[i as usize]) & 0xff);
    }

    // 3. resolve references, synthesize import stubs, apply relocations
    let mut stubs: FastMap<&[u8], u64> = FastMap::default();
    let mut out: Vec<u64> = Vec::new();
    for obj in objs {
        for &(off, len) in &obj.refs {
            let name = &obj.strtab[off as usize..(off + len) as usize];
            let target = match table.get(name) {
                Some(&id) => addr[id as usize],
                None => {
                    let n = stubs.len() as u64;
                    *stubs.entry(name).or_insert(0x2_0000_0000 + n * 12)
                }
            };
            out.push(target ^ (out.len() as u64));
        }
    }

    let mut h = mix(dups, stubs.len() as u64);
    for (i, &v) in out.iter().enumerate().step_by(7) {
        h = mix(h, v ^ i as u64);
    }
    mix(h, mix(cursor, all[order[order.len() / 2] as usize].len() as u64))
}
