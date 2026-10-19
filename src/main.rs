mod compare;
#[cfg(test)]
mod json;
mod sysinfo;
mod util;
mod work;

use std::path::PathBuf;
use std::time::Instant;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Bump whenever test workloads or reference values change: results of different
/// score versions are not comparable and live in separate results/v<N>/ folders.
const SCORE_VERSION: u32 = 1;

struct Ctx {
    dir: PathBuf,
}

struct Test {
    id: &'static str,
    name: &'static str,
    models: &'static str,
    mt: bool,
    /// Upper bound on worker threads (I/O tests saturate the file system, not the CPU).
    cap: usize,
    /// Median time on the reference machine (scores 1000 there).
    ref_secs: f64,
    /// Expected checksum: identical on every platform, guards against broken ports.
    expect: u64,
    setup: fn(&Ctx) -> Box<dyn FnMut(usize) -> u64>,
}

const TESTS: &[Test] = &[
    Test { id: "compile_st", name: "Compiler frontend, 1 thread", models: "incremental build: kotlinc / swiftc / dart frontend on one module",
        mt: false, cap: usize::MAX, ref_secs: 1.1669, expect: 0x3f4744cd600b3f46, setup: |_| work::compile::setup_st() },
    Test { id: "compile_mt", name: "Compiler frontend, all threads", models: "clean build: parallel Gradle workers / swift-driver / xcodebuild",
        mt: true, cap: usize::MAX, ref_secs: 1.1293, expect: 0x78c8b005c07078bc, setup: |_| work::compile::setup_mt() },
    Test { id: "link_st", name: "Linker (symbols, relocations)", models: "ld64 / lld / R8 / dex merge",
        mt: false, cap: usize::MAX, ref_secs: 1.1224, expect: 0xdc3be5d377577988, setup: |_| work::link::setup() },
    Test { id: "alloc_mt", name: "Managed runtime (alloc/GC)", models: "Gradle & Kotlin daemons, Dart VM, analysis servers",
        mt: true, cap: usize::MAX, ref_secs: 0.9760, expect: 0x128796ccb0fc3c14, setup: |_| work::alloc::setup() },
    Test { id: "compress_mt", name: "Packaging (LZ77)", models: "APK / AAB / IPA zipping, asset compression",
        mt: true, cap: usize::MAX, ref_secs: 0.9430, expect: 0xe00454a06dab9bc0, setup: |_| work::compress::setup() },
    Test { id: "hash_mt", name: "Hashing (SHA-256)", models: "build cache keys, code signing, checksums",
        mt: true, cap: usize::MAX, ref_secs: 1.0906, expect: 0x933d6d8614a2858c, setup: |_| work::hash::setup() },
    Test { id: "fs_small", name: "Small files I/O", models: "build/, DerivedData, .dart_tool, Gradle caches",
        mt: true, cap: 4, ref_secs: 0.7991, expect: 0xbdef26544ac07e68, setup: |c| work::fs::setup_small(c.dir.clone()) },
    Test { id: "disk_seq", name: "Sequential write 1 GiB + fsync", models: "large artifacts, emulator images, archives",
        mt: false, cap: usize::MAX, ref_secs: 0.3395, expect: 1 << 30, setup: |c| work::fs::setup_seq(c.dir.clone()) },
    Test { id: "mem_bw", name: "Memory bandwidth", models: "emulators/simulators, big heaps, large binaries",
        mt: true, cap: usize::MAX, ref_secs: 0.8068, expect: 0x4583e0cdb40880c6, setup: |_| work::mem::setup_bw() },
    Test { id: "mem_lat", name: "Memory latency", models: "pointer-heavy compiler and VM data structures",
        mt: false, cap: usize::MAX, ref_secs: 1.3056, expect: 0x0000000001127d28, setup: |_| work::mem::setup_lat() },
];

type Weights = &'static [(&'static str, f64)];

const PROFILES: &[(&str, Weights)] = &[
    ("Android", &[("compile_mt", 3.0), ("alloc_mt", 2.5), ("compile_st", 1.5), ("fs_small", 1.5), ("compress_mt", 1.0),
                  ("link_st", 0.5), ("hash_mt", 0.5), ("mem_bw", 0.5), ("disk_seq", 0.5)]),
    ("iOS", &[("compile_mt", 3.0), ("compile_st", 2.0), ("link_st", 2.0), ("fs_small", 1.0), ("hash_mt", 1.0),
              ("alloc_mt", 0.5), ("mem_lat", 0.5), ("disk_seq", 0.5)]),
    ("Flutter", &[("compile_st", 2.0), ("alloc_mt", 2.0), ("compile_mt", 2.0), ("fs_small", 1.5), ("link_st", 1.0),
                  ("compress_mt", 0.5), ("mem_lat", 0.5), ("mem_bw", 0.5)]),
];

const CATEGORIES: &[(&str, Weights)] = &[
    ("Single-core", &[("compile_st", 1.0), ("link_st", 1.0), ("mem_lat", 0.5)]),
    ("Multi-core", &[("compile_mt", 1.0), ("alloc_mt", 1.0), ("compress_mt", 1.0), ("hash_mt", 1.0)]),
    ("Memory", &[("mem_bw", 1.0), ("mem_lat", 1.0)]),
    ("Storage", &[("fs_small", 1.0), ("disk_seq", 1.0)]),
];

struct Opts {
    runs: usize,
    warmup: bool,
    threads: usize,
    only: Vec<String>,
    skip: Vec<String>,
    json: Option<PathBuf>,
    dir: PathBuf,
    calibrate: bool,
    save: bool,
    compare: compare::Key,
    leaderboard: bool,
}

struct Outcome {
    test: &'static Test,
    times: Vec<f64>,
    median: f64,
    checksum: u64,
    valid: bool,
    score: f64,
}

fn usage() -> ! {
    println!(
        "mobench {VERSION} — benchmark a computer for mobile development (Android / iOS / Flutter)

USAGE: mobench [options]

  -r, --runs N       measured runs per test, median is used (default 3)
  -q, --quick        1 run, no warm-up (rough numbers)
  -t, --threads N    worker threads for multi-threaded tests (default: all)
      --only a,b     run only these tests
      --skip a,b     skip these tests (e.g. --skip disk_seq)
      --dir PATH     directory for disk tests (default: system temp; point it at your projects drive)
      --json FILE    also write results as JSON
      --save         save the result to results/v<N>/<cpu>/ to share it via a pull request
      --compare K    rank by: total (default), android, ios, flutter
      --leaderboard  show the ranking of submitted results without running anything
      --list         list tests and exit
  -h, --help         this help

Score: 1000 = reference machine (Apple M4 Pro, 14 cores). Higher is better; 2000 = twice as fast."
    );
    std::process::exit(0)
}

fn parse_args() -> Opts {
    let mut o = Opts {
        runs: 3,
        warmup: true,
        threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        only: vec![],
        skip: vec![],
        json: None,
        dir: std::env::temp_dir(),
        calibrate: false,
        save: false,
        compare: compare::Key::Total,
        leaderboard: false,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let value = |i: &mut usize| -> String {
        *i += 1;
        args.get(*i).cloned().unwrap_or_else(|| {
            eprintln!("missing value for {}", args[*i - 1]);
            std::process::exit(2)
        })
    };
    let number = |s: String| -> usize {
        s.parse().ok().filter(|&n| n > 0).unwrap_or_else(|| {
            eprintln!("expected a positive number, got '{}'", s);
            std::process::exit(2)
        })
    };
    let list = |s: String| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
    while i < args.len() {
        match args[i].as_str() {
            "-r" | "--runs" => o.runs = number(value(&mut i)),
            "-q" | "--quick" => {
                o.runs = 1;
                o.warmup = false;
            }
            "-t" | "--threads" => o.threads = number(value(&mut i)),
            "--only" => o.only = list(value(&mut i)),
            "--skip" => o.skip = list(value(&mut i)),
            "--json" => o.json = Some(PathBuf::from(value(&mut i))),
            "--dir" => o.dir = PathBuf::from(value(&mut i)),
            "--calibrate" => o.calibrate = true,
            "--save" => o.save = true,
            "--leaderboard" => o.leaderboard = true,
            "--compare" => {
                let v = value(&mut i);
                o.compare = compare::Key::parse(&v).unwrap_or_else(|| {
                    eprintln!("--compare expects total, android, ios or flutter, got '{}'", v);
                    std::process::exit(2)
                })
            }
            "--list" => {
                for t in TESTS {
                    println!("{:<12} {:<4} {:<34} {}", t.id, if t.mt { "MT" } else { "1T" }, t.name, t.models);
                }
                std::process::exit(0)
            }
            "-h" | "--help" => usage(),
            "-V" | "--version" => {
                println!("mobench {VERSION}");
                std::process::exit(0)
            }
            other => {
                eprintln!("unknown option '{}', see --help", other);
                std::process::exit(2)
            }
        }
        i += 1;
    }
    for id in o.only.iter().chain(&o.skip) {
        if !TESTS.iter().any(|t| t.id == id) {
            eprintln!("unknown test '{}', see --list", id);
            std::process::exit(2)
        }
    }
    o
}

fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = s.len();
    if n % 2 == 1 { s[n / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 }
}

/// Weighted geometric mean over the tests that were actually run.
fn composite(results: &[Outcome], weights: Weights) -> Option<(f64, bool)> {
    let (mut sum, mut wsum, mut present) = (0.0, 0.0, 0);
    for &(id, w) in weights {
        if let Some(r) = results.iter().find(|r| r.test.id == id) {
            sum += w * r.score.ln();
            wsum += w;
            present += 1;
        }
    }
    if present == 0 { None } else { Some(((sum / wsum).exp(), present < weights.len())) }
}

fn bar(score: f64) -> String {
    let n = ((score / 100.0).round() as usize).min(30);
    "█".repeat(n)
}

fn cores_text(info: &sysinfo::SysInfo) -> String {
    match &info.cores {
        Some(c) => format!("{} cores, {} threads", c, info.threads),
        None => format!("{} threads", info.threads),
    }
}

/// Final shareable card: hardware + OS + score.
fn summary_card(info: &sysinfo::SysInfo, total: f64, partial: bool, profs: &[(String, f64)], cats: &[(String, f64)]) {
    const MAX: usize = 72;
    let fit = |s: String| -> String {
        if s.chars().count() <= MAX { s } else { s.chars().take(MAX - 1).collect::<String>() + "…" }
    };
    let join = |v: &[(String, f64)]| v.iter().map(|(k, s)| format!("{} {:.0}", k, s)).collect::<Vec<_>>().join("   ");

    let mut body = vec![String::new()];
    body.push(fit(format!("Device   {}", info.model.as_deref().unwrap_or("unknown"))));
    body.push(fit(format!("CPU      {}", info.cpu)));
    body.push(fit(format!("Cores    {}", cores_text(info))));
    if let Some(gb) = info.ram_gb {
        body.push(format!("RAM      {:.0} GB", gb));
    }
    body.push(fit(format!("OS       {} · {}", info.os, info.arch)));
    body.push(String::new());
    body.push(fit(join(profs)));
    body.push(fit(join(cats)));

    let title = "MOBILE DEV SCORE";
    let score = format!("{:.0}{}", total, if partial { " (partial)" } else { "" });
    let width = body.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(title.len() + score.chars().count() + 6).max(44);
    let pad = |l: &str| " ".repeat(width - l.chars().count());

    println!("  ╭{}╮", "─".repeat(width + 4));
    println!("  │  {}{}{}  │", title, " ".repeat(width - title.len() - score.chars().count()), score);
    for l in &body {
        println!("  │  {}{}  │", l, pad(l));
    }
    println!("  ╰{}╯", "─".repeat(width + 4));
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn main() {
    let opts = parse_args();
    if opts.leaderboard {
        if !compare::print(SCORE_VERSION, opts.compare, None) {
            println!("No results for score version {} yet. Run `mobench --save` and open a pull request.", SCORE_VERSION);
        }
        return;
    }
    let info = sysinfo::collect();
    let ctx = Ctx { dir: opts.dir.clone() };

    println!("mobench {VERSION} — mobile development benchmark");
    if let Some(m) = &info.model {
        println!("  Device  {}", m);
    }
    println!("  OS      {} · {}", info.os, info.arch);
    println!("  CPU     {} — {} (using {} threads)", info.cpu, cores_text(&info), opts.threads);
    if let Some(gb) = info.ram_gb {
        println!("  RAM     {:.0} GB", gb);
    }
    println!("  Disk    {}", opts.dir.display());
    if info.on_battery == Some(true) {
        println!("  ⚠ running on battery — plug in the charger for representative results");
    }
    println!();

    let selected: Vec<&'static Test> = TESTS
        .iter()
        .filter(|t| opts.only.is_empty() || opts.only.iter().any(|x| x == t.id))
        .filter(|t| !opts.skip.iter().any(|x| x == t.id))
        .collect();

    let mut results: Vec<Outcome> = Vec::new();
    let started = Instant::now();
    for (n, t) in selected.iter().enumerate() {
        eprint!("\r\x1b[2K  [{}/{}] {} — preparing...", n + 1, selected.len(), t.name);
        let mut run = (t.setup)(&ctx);
        let threads = if t.mt { opts.threads.min(t.cap) } else { 1 };
        if opts.warmup {
            eprint!("\r\x1b[2K  [{}/{}] {} — warm-up...", n + 1, selected.len(), t.name);
            std::hint::black_box(run(threads));
        }
        let mut times = Vec::new();
        let mut sums = Vec::new();
        for k in 0..opts.runs {
            eprint!("\r\x1b[2K  [{}/{}] {} — run {}/{}...", n + 1, selected.len(), t.name, k + 1, opts.runs);
            let start = Instant::now();
            let sum = std::hint::black_box(run(threads));
            times.push(start.elapsed().as_secs_f64());
            sums.push(sum);
        }
        drop(run);
        let med = median(&times);
        let consistent = sums.iter().all(|&s| s == sums[0]);
        let valid = consistent && (t.expect == 0 || sums[0] == t.expect);
        results.push(Outcome { test: t, median: med, checksum: sums[0], valid, score: 1000.0 * t.ref_secs / med, times });
    }
    eprint!("\r\x1b[2K");

    if opts.calibrate {
        for r in &results {
            println!("{:<12} ref_secs: {:.4}, expect: 0x{:016x}", r.test.id, r.median, r.checksum);
        }
        return;
    }

    println!("  {:<12} {:<34} {:>3} {:>9} {:>6}  {:>6}", "test", "", "thr", "time", "±", "score");
    println!("  {}", "─".repeat(78));
    for r in &results {
        let spread = if r.times.len() > 1 {
            let max = r.times.iter().cloned().fold(f64::MIN, f64::max);
            let min = r.times.iter().cloned().fold(f64::MAX, f64::min);
            format!("{:.1}%", (max - min) / r.median * 100.0)
        } else {
            "-".into()
        };
        println!(
            "  {:<12} {:<34} {:>3} {:>8.3}s {:>6}  {:>6.0} {}",
            r.test.id,
            r.test.name,
            if r.test.mt { opts.threads.min(r.test.cap) } else { 1 },
            r.median,
            spread,
            r.score,
            if r.valid { "" } else { "  ✗ CHECKSUM MISMATCH" }
        );
    }

    let print_group = |title: &str, groups: &[(&str, Weights)]| -> Vec<(String, f64)> {
        println!("\n  {}", title);
        let mut out = Vec::new();
        for &(name, w) in groups {
            if let Some((s, partial)) = composite(&results, w) {
                println!("    {:<12} {:>6.0}  {}{}", name, s, bar(s), if partial { "  (partial)" } else { "" });
                out.push((name.to_string(), s));
            }
        }
        out
    };
    let cats = print_group("Categories", CATEGORIES);
    let profs = print_group("Platforms", PROFILES);

    let total = if profs.is_empty() {
        None
    } else {
        Some((profs.iter().map(|(_, s)| s.ln()).sum::<f64>() / profs.len() as f64).exp())
    };
    let partial = selected.len() < TESTS.len();
    if let Some(t) = total {
        println!();
        summary_card(&info, t, partial, &profs, &cats);
    }
    if std::env::consts::OS != "macos" {
        println!("  note: iOS score shows how fast this hardware would be; iOS builds require macOS.");
    }
    if results.iter().any(|r| !r.valid) {
        println!("  ✗ some tests produced wrong checksums — results are invalid (please report a bug).");
    }
    println!("  1000 = Apple M4 Pro (14 cores). Total time {:.0}s.", started.elapsed().as_secs_f64());

    let (timestamp, file_stamp) = utc_now();
    let report = Report { info: &info, opts: &opts, results: &results, cats: &cats, profs: &profs, total, partial, timestamp: &timestamp };

    if let (Some(t), false) = (total, partial) {
        let p = |name: &str| profs.iter().find(|(k, _)| k == name).map(|(_, s)| *s).unwrap_or(0.0);
        let mine = compare::Mine {
            cpu: info.cpu.clone(),
            cores: info.cores.clone(),
            threads: info.threads,
            total: t,
            android: p("Android"),
            ios: p("iOS"),
            flutter: p("Flutter"),
        };
        if !compare::print(SCORE_VERSION, opts.compare, Some(&mine)) {
            println!("\n  No shared results to compare with yet — be the first: run with --save.");
        }
    }

    if let Some(path) = &opts.json {
        match std::fs::write(path, report.to_json()) {
            Ok(_) => println!("  JSON written to {}", path.display()),
            Err(e) => eprintln!("  cannot write {}: {}", path.display(), e),
        }
    }
    if opts.save {
        save(&report, &file_stamp);
    }
}

struct Report<'a> {
    info: &'a sysinfo::SysInfo,
    opts: &'a Opts,
    results: &'a [Outcome],
    cats: &'a [(String, f64)],
    profs: &'a [(String, f64)],
    total: Option<f64>,
    partial: bool,
    timestamp: &'a str,
}

impl Report<'_> {
    fn to_json(&self) -> String {
        let (info, opts) = (self.info, self.opts);
        let mut j = String::from("{\n");
        j += &format!("  \"version\": {},\n", json_str(VERSION));
        j += &format!("  \"score_version\": {},\n", SCORE_VERSION);
        j += &format!("  \"timestamp\": {},\n", json_str(self.timestamp));
        j += &format!("  \"runs\": {},\n  \"warmup\": {},\n", opts.runs, opts.warmup);
        j += &format!(
            "  \"system\": {{\"os\": {}, \"arch\": {}, \"model\": {}, \"cpu\": {}, \"cores\": {}, \"threads\": {}, \"threads_used\": {}, \"ram_gb\": {}, \"on_battery\": {}}},\n",
            json_str(&info.os),
            json_str(info.arch),
            info.model.as_deref().map(json_str).unwrap_or("null".into()),
            json_str(&info.cpu),
            info.cores.as_deref().map(json_str).unwrap_or("null".into()),
            info.threads,
            opts.threads,
            info.ram_gb.map(|g| format!("{:.1}", g)).unwrap_or("null".into()),
            info.on_battery.map(|b| b.to_string()).unwrap_or("null".into())
        );
        j += "  \"tests\": [\n";
        for (i, r) in self.results.iter().enumerate() {
            let times: Vec<String> = r.times.iter().map(|t| format!("{:.4}", t)).collect();
            j += &format!(
                "    {{\"id\": {}, \"median_s\": {:.4}, \"times_s\": [{}], \"score\": {:.1}, \"valid\": {}}}{}\n",
                json_str(r.test.id),
                r.median,
                times.join(", "),
                r.score,
                r.valid,
                if i + 1 < self.results.len() { "," } else { "" }
            );
        }
        j += "  ],\n";
        let obj = |v: &[(String, f64)]| v.iter().map(|(k, s)| format!("{}: {:.1}", json_str(k), s)).collect::<Vec<_>>().join(", ");
        j += &format!("  \"categories\": {{{}}},\n", obj(self.cats));
        j += &format!("  \"platforms\": {{{}}},\n", obj(self.profs));
        j += &format!("  \"total\": {},\n", self.total.map(|t| format!("{:.1}", t)).unwrap_or("null".into()));
        j += &format!("  \"partial\": {}\n}}\n", self.partial);
        j
    }
}

/// Writes results/v<N>/<cpu-slug>/<UTC time>-<random>.json. One file per submission, so two people
/// with the same hardware never touch the same file and their pull requests never conflict.
fn save(report: &Report, file_stamp: &str) {
    let reason = if report.partial {
        Some("only full runs can be shared (remove --only/--skip)")
    } else if !report.opts.warmup || report.opts.runs < 3 {
        Some("quick runs can't be shared (run without --quick, at least 3 runs)")
    } else if report.opts.threads != report.info.threads {
        Some("runs with --threads can't be shared")
    } else if report.results.iter().any(|r| !r.valid) {
        Some("some checksums are wrong")
    } else if report.info.on_battery == Some(true) {
        Some("measured on battery — plug in the charger and run again")
    } else {
        None
    };
    if let Some(r) = reason {
        println!("\n  ✗ not saved: {}.", r);
        return;
    }
    let slug: String = report
        .info
        .cpu
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && !matches!(*w, "r" | "tm" | "c" | "cpu" | "processor" | "with"))
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() { "unknown-cpu".to_string() } else { slug };
    let json = report.to_json();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let id = util::mix(util::mix(nanos, std::process::id() as u64), util::fnv1a(json.as_bytes())) as u32;
    let dir = PathBuf::from("results").join(format!("v{}", SCORE_VERSION)).join(&slug);
    let path = dir.join(format!("{}-{:08x}.json", file_stamp, id));
    let written = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, json));
    match written {
        Ok(_) => {
            println!("\n  ✓ Saved {}", path.display());
            println!("    To share it, commit this file and open a pull request:");
            println!("      git checkout -b result-{}-{:08x}", slug, id);
            println!("      git add {} && git commit -m \"Add result: {}\"", path.display().to_string().replace('\\', "/"), report.info.cpu);
            println!("      git push -u origin HEAD   # then open the pull request on GitHub");
        }
        Err(e) => println!("\n  ✗ cannot save {}: {}", path.display(), e),
    }
}

/// (ISO-8601 UTC timestamp, file-name-safe variant), no external crates.
fn utc_now() -> (String, String) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let (hh, mm, ss) = (sod / 3600, sod % 3600 / 60, sod % 60);
    (
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, hh, mm, ss),
        format!("{:04}-{:02}-{:02}T{:02}-{:02}-{:02}Z", y, m, d, hh, mm, ss),
    )
}
