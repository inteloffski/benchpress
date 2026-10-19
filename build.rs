//! Embeds every `results/**/*.json` into the binary as a static table for the comparison view.
//! There is deliberately no shared index file in the repo: each submission is its own file,
//! so concurrent pull requests never conflict. Invalid files never break the build; they are
//! listed in `REJECTED`, and a unit test (run in CI on every PR) fails if any exist.

#[path = "src/json.rs"]
#[allow(dead_code)]
mod json;

use json::Json;
use std::fmt::Write;
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().map_or(false, |x| x == "json") {
            out.push(p);
        }
    }
}

struct Rec {
    score_version: u32,
    cpu: String,
    cores: String,
    threads: u32,
    model: String,
    os: String,
    arch: String,
    total: f64,
    android: f64,
    ios: f64,
    flutter: f64,
}

fn validate(path: &Path, root: &Path) -> Result<Rec, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let j = json::parse(&text)?;
    let num = |v: Option<&Json>, what: &str| v.and_then(Json::as_f64).ok_or(format!("missing number '{}'", what));
    let s = |v: Option<&Json>| v.and_then(Json::as_str).map(str::to_string);

    let score_version = num(j.get("score_version"), "score_version")? as u32;
    let dir_version = path.strip_prefix(root).ok().and_then(|p| p.components().next()).map(|c| c.as_os_str().to_string_lossy().to_string());
    if dir_version.as_deref() != Some(&format!("v{}", score_version)) {
        return Err(format!("score_version {} does not match its folder (expected results/v{}/...)", score_version, score_version));
    }
    if j.get("partial").and_then(Json::as_bool) != Some(false) {
        return Err("partial run".into());
    }
    if j.get("warmup").and_then(Json::as_bool) != Some(true) || num(j.get("runs"), "runs")? < 3.0 {
        return Err("quick run (needs warm-up and at least 3 runs)".into());
    }
    let tests = j.get("tests").and_then(Json::as_arr).ok_or("missing 'tests'")?;
    if tests.iter().any(|t| t.get("valid").and_then(Json::as_bool) != Some(true)) {
        return Err("contains tests with invalid checksums".into());
    }

    let sys = j.get("system").ok_or("missing 'system'")?;
    if sys.get("on_battery").and_then(Json::as_bool) == Some(true) {
        return Err("measured on battery".into());
    }
    let threads = num(sys.get("threads"), "system.threads")?;
    if num(sys.get("threads_used"), "system.threads_used")? != threads {
        return Err("not all CPU threads were used (--threads)".into());
    }
    let plat = j.get("platforms").ok_or("missing 'platforms'")?;
    let score = |v: Option<&Json>, what: &str| -> Result<f64, String> {
        let x = num(v, what)?;
        if x > 0.0 && x < 1e6 { Ok(x) } else { Err(format!("'{}' out of range", what)) }
    };
    Ok(Rec {
        score_version,
        cpu: s(sys.get("cpu")).ok_or("missing 'system.cpu'")?,
        cores: s(sys.get("cores")).unwrap_or_default(),
        threads: threads as u32,
        model: s(sys.get("model")).unwrap_or_default(),
        os: s(sys.get("os")).unwrap_or_default(),
        arch: s(sys.get("arch")).unwrap_or_default(),
        total: score(j.get("total"), "total")?,
        android: score(plat.get("Android"), "platforms.Android")?,
        ios: score(plat.get("iOS"), "platforms.iOS")?,
        flutter: score(plat.get("Flutter"), "platforms.Flutter")?,
    })
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("results");
    println!("cargo:rerun-if-changed=results");
    println!("cargo:rerun-if-changed=src/json.rs");

    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();

    let mut recs = String::from("pub static RECORDS: &[Record] = &[\n");
    let mut rejected = String::from("#[allow(dead_code)]\npub static REJECTED: &[(&str, &str)] = &[\n");
    for path in &files {
        let rel = path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap_or(path).to_string_lossy().replace('\\', "/");
        match validate(path, &root) {
            Ok(r) => {
                let _ = writeln!(
                    recs,
                    "    Record {{ file: {:?}, score_version: {}, cpu: {:?}, cores: {:?}, threads: {}, model: {:?}, os: {:?}, arch: {:?}, total: {:?}, android: {:?}, ios: {:?}, flutter: {:?} }},",
                    rel, r.score_version, r.cpu, r.cores, r.threads, r.model, r.os, r.arch, r.total, r.android, r.ios, r.flutter
                );
            }
            Err(e) => {
                println!("cargo:warning=skipping {}: {}", rel, e);
                let _ = writeln!(rejected, "    ({:?}, {:?}),", rel, e);
            }
        }
    }
    recs.push_str("];\n");
    rejected.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("results.rs");
    std::fs::write(out, recs + &rejected).unwrap();
}
