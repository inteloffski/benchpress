<p align="center">
  <b>English</b> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a>
</p>

# mobench

A cross-platform CLI benchmark that rates a computer for mobile development (Android / iOS / Flutter).
It is a single binary with no dependencies, and you don't need Xcode, the Android SDK or Flutter to run it.
It runs on macOS, Linux and Windows, on x86_64 and ARM64.

Synthetic number crunching like matrix multiplication says little about build speed, so each test
models a specific stage of a mobile build instead. Every test computes a checksum that must be identical
on all platforms. That shows each machine did exactly the same work.

| test | what it models | threads |
|---|---|---|
| `compile_st` | compiler frontend (lexer → parser → const-fold → type-check → bytecode) on generated Kotlin-like code; incremental build of one module | 1 |
| `compile_mt` | the same on 2400 files in parallel; clean build (Gradle workers, swift-driver) | all |
| `link_st` | linking: global table of ~800k symbols, sorting, address layout, ~1.9M relocations (ld64, lld, R8) | 1 |
| `alloc_mt` | managed runtime: many short-lived objects, pointer graphs, string-keyed hash maps (Gradle/Kotlin daemon, Dart VM) | all |
| `compress_mt` | APK/AAB/IPA packaging: LZ77 with hash chains (the core of deflate) plus verifying decompression | all |
| `hash_mt` | build cache keys, code signing: SHA-256 | all |
| `fs_small` | build intermediates: creating, renaming, scanning, reading and deleting ~7.7k small files | ≤4 |
| `disk_seq` | sequential 1 GiB write + fsync | 1 |
| `mem_bw` | memory bandwidth (STREAM triad) | all |
| `mem_lat` | memory latency (pointer chasing over 128 MiB) | 1 |

## Scoring

- Test score = `1000 × reference_time / median_time`. **1000 is the reference machine (Apple M4 Pro, 14 cores)**;
  2000 means twice as fast.
- **Categories** (Single-core, Multi-core, Memory, Storage) and **platforms** (Android, iOS, Flutter)
  are weighted geometric means of the test scores. The weights live in `PROFILES` and `CATEGORIES` in
  `src/main.rs`. For example, Android leans on parallel compilation and the managed runtime (Gradle),
  while iOS leans on single-threaded compilation and the linker.
- **MOBILE DEV SCORE** is the geometric mean of the three platform scores.

At the end of a run, mobench prints a summary card with the hardware, the OS version and the scores:

```
  ╭──────────────────────────────────────────────────────────────────╮
  │  MOBILE DEV SCORE                                           994  │
  │                                                                  │
  │  Device   MacBook Pro (Mac16,7)                                  │
  │  CPU      Apple M4 Pro                                           │
  │  Cores    10P + 4E cores, 14 threads                             │
  │  RAM      24 GB                                                  │
  │  OS       macOS 26.6.2 (build 25G83) · aarch64                   │
  │                                                                  │
  │  Android 997   iOS 993   Flutter 994                             │
  │  Single-core 977   Multi-core 996   Memory 983   Storage 1022    │
  ╰──────────────────────────────────────────────────────────────────╯
```

## Download

| OS | file |
|---|---|
| macOS, Apple Silicon (M1 and newer) | [`mobench-macos-arm64.tar.gz`](https://github.com/inteloffski/benchpress/releases/latest/download/mobench-macos-arm64.tar.gz) |
| macOS, Intel | [`mobench-macos-x86_64.tar.gz`](https://github.com/inteloffski/benchpress/releases/latest/download/mobench-macos-x86_64.tar.gz) |
| **Windows x64 — most PCs** | [`mobench-windows-x86_64.zip`](https://github.com/inteloffski/benchpress/releases/latest/download/mobench-windows-x86_64.zip) |
| Windows ARM — Snapdragon laptops only | [`mobench-windows-arm64.zip`](https://github.com/inteloffski/benchpress/releases/latest/download/mobench-windows-arm64.zip) |
| Linux x64 | [`mobench-linux-x86_64.tar.gz`](https://github.com/inteloffski/benchpress/releases/latest/download/mobench-linux-x86_64.tar.gz) |
| Linux ARM64 | [`mobench-linux-arm64.tar.gz`](https://github.com/inteloffski/benchpress/releases/latest/download/mobench-linux-arm64.tar.gz) |

Not sure which Windows file you need? Open Settings → System → About and check "System type":
"x64-based processor" means x64, "ARM-based processor" means ARM. The wrong one fails with
"This app can't run on your PC".

```sh
# macOS / Linux: replace macos-arm64 with your platform from the table
curl -L https://github.com/inteloffski/benchpress/releases/latest/download/mobench-macos-arm64.tar.gz | tar xz
./mobench
```

```powershell
# Windows (PowerShell)
Invoke-WebRequest https://github.com/inteloffski/benchpress/releases/latest/download/mobench-windows-x86_64.zip -OutFile mobench.zip
Expand-Archive mobench.zip -DestinationPath .
.\mobench.exe
```

The binaries are not signed. If you download them through a browser, macOS may block the file;
run `xattr -d com.apple.quarantine mobench` to unblock it. Downloads via `curl` are not affected.
Windows SmartScreen may show a warning: click "More info" → "Run anyway".

## Usage

```sh
git clone https://github.com/inteloffski/benchpress && cd benchpress
cargo build --release
./target/release/mobench                 # full run: warm-up + 3 runs, ~40 s on an M4 Pro
./target/release/mobench --quick        # 1 run, no warm-up
./target/release/mobench --dir D:\work  # run the disk tests on a specific drive
./target/release/mobench --json result.json
./target/release/mobench --list
```

`.github/workflows/release.yml` builds binaries for every OS and architecture when you push a `v*` tag.

## Comparing with other machines

After each run mobench shows where your machine ranks among results shared by other people
(median per CPU, `n` = number of results). `--compare android|ios|flutter` ranks by platform,
and `mobench --leaderboard` prints the ranking without running anything.

### Sharing your result

1. Fork this repository, clone your fork and build it.
2. Plug in the charger and run `./target/release/mobench --save`. A full run is required,
   so `--quick`, `--only`, `--skip` and `--threads` are not allowed.
3. Commit the new file and open a pull request. mobench prints the exact commands.

Every result is a **separate new file**: `results/v1/<cpu>/<UTC time>-<random id>.json`.
Two people with the same hardware never touch the same file, so pull requests never conflict
and both results are kept. Results with the same CPU are combined by median.
CI checks every file in a pull request: invalid checksums, quick or partial runs, battery power,
or a mismatched score version fail the check.
`v1` is the scoring version. If tests or reference values change, new results go to `v2`
and are never mixed with old ones.

## Getting fair results

- Keep laptops on the charger. mobench warns you when it detects battery power.
- Close the IDE, emulators and browser.
- To check for thermal throttling, run `--runs 10` and look at the spread (the `±` column).
- On Windows, `fs_small` depends heavily on Defender. That is a real factor in build speed, so compare
  against a run in a folder excluded from scanning.

## Recalibrating the reference

If you change the amount of work in any test, recalibrate the reference: run `mobench --calibrate --runs 5`
on the reference machine, then copy the resulting `ref_secs` and `expect` values into `TESTS` in `src/main.rs`.
Old results are no longer comparable after that, so bump the version.
