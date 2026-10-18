//! File-system model: build intermediates (build/, DerivedData, .dart_tool, Gradle caches).
//! Thousands of small files: create (some via temp+rename like atomic writers), scan, stat,
//! read, delete. Measures OS + file system + antivirus + SSD together — as a real build sees them.

use crate::util::{fnv1a, mix, par_jobs, Rng};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const MODULES: usize = 64;
const PKGS: usize = 4;
const FILES: usize = 30;
const SEQ_SIZE: usize = 1 << 30;
const SEQ_CHUNK: usize = 8 << 20;

fn unique_dir(base: &Path, tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    base.join(format!("mobench-{}-{}-{}", tag, std::process::id(), nanos))
}

pub fn setup_small(base: PathBuf) -> Box<dyn FnMut(usize) -> u64> {
    let mut content = vec![0u8; 64 * 1024];
    Rng::new(5).fill(&mut content);
    Box::new(move |threads| {
        let root = unique_dir(&base, "fs");
        fs::create_dir_all(&root).expect("cannot create benchmark dir");
        let sum = par_jobs(threads, MODULES, |m| module_job(&root, m, &content).expect("file system error"));
        let _ = fs::remove_dir_all(&root);
        sum
    })
}

fn module_job(root: &Path, m: usize, content: &[u8]) -> std::io::Result<u64> {
    let mut r = Rng::new(m as u64);
    let dir = root.join(format!("module{}", m));
    for p in 0..PKGS {
        let pkg = dir.join("build").join("intermediates").join(format!("pkg{}", p));
        fs::create_dir_all(&pkg)?;
        for f in 0..FILES {
            let len = r.range(512, 16 * 1024);
            let off = r.below(content.len() - len);
            let path = pkg.join(format!("Class{}$Inner{}.class", f, p));
            if f % 4 == 0 {
                let tmp = pkg.join(format!(".Class{}.tmp", f));
                fs::write(&tmp, &content[off..off + len])?;
                fs::rename(&tmp, &path)?;
            } else {
                fs::write(&path, &content[off..off + len])?;
            }
        }
    }

    // up-to-date check + read, like an incremental build scanning its outputs
    let mut h = 0u64;
    let mut count = 0u64;
    for p in 0..PKGS {
        let pkg = dir.join("build").join("intermediates").join(format!("pkg{}", p));
        for entry in fs::read_dir(&pkg)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            let data = fs::read(entry.path())?;
            if data.len() as u64 != meta.len() {
                return Err(std::io::Error::new(std::io::ErrorKind::Other, "size mismatch"));
            }
            h = h.wrapping_add(fnv1a(&data));
            count += 1;
        }
    }
    fs::remove_dir_all(&dir)?;
    Ok(mix(h, count))
}

pub fn setup_seq(base: PathBuf) -> Box<dyn FnMut(usize) -> u64> {
    let mut chunk = vec![0u8; SEQ_CHUNK];
    Rng::new(11).fill(&mut chunk);
    Box::new(move |_| {
        let path = unique_dir(&base, "seq");
        let mut f = fs::File::create(&path).expect("cannot create benchmark file");
        for i in 0..SEQ_SIZE / SEQ_CHUNK {
            chunk[0] = i as u8; // defeat dedup
            f.write_all(&chunk).expect("write failed");
        }
        f.sync_all().expect("fsync failed");
        drop(f);
        let len = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let _ = fs::remove_file(&path);
        len
    })
}
