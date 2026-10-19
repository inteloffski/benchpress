//! "How you compare": results submitted to `results/` are embedded at build time (see build.rs)
//! and grouped by CPU. Several submissions of the same hardware are combined by median, so
//! duplicates and occasional outliers do not distort the ranking.

pub struct Record {
    #[allow(dead_code)]
    pub file: &'static str,
    pub score_version: u32,
    pub cpu: &'static str,
    pub cores: &'static str,
    pub threads: u32,
    pub model: &'static str,
    #[allow(dead_code)]
    pub os: &'static str,
    #[allow(dead_code)]
    pub arch: &'static str,
    pub total: f64,
    pub android: f64,
    pub ios: f64,
    pub flutter: f64,
}

include!(concat!(env!("OUT_DIR"), "/results.rs"));

#[derive(Clone, Copy, PartialEq)]
pub enum Key {
    Total,
    Android,
    Ios,
    Flutter,
}

impl Key {
    pub fn parse(s: &str) -> Option<Key> {
        match s.to_ascii_lowercase().as_str() {
            "total" => Some(Key::Total),
            "android" => Some(Key::Android),
            "ios" => Some(Key::Ios),
            "flutter" => Some(Key::Flutter),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Key::Total => "MOBILE DEV SCORE",
            Key::Android => "Android score",
            Key::Ios => "iOS score",
            Key::Flutter => "Flutter score",
        }
    }

    fn of(self, r: &Record) -> f64 {
        match self {
            Key::Total => r.total,
            Key::Android => r.android,
            Key::Ios => r.ios,
            Key::Flutter => r.flutter,
        }
    }
}

/// This machine's result, shown among the others.
pub struct Mine {
    pub cpu: String,
    pub cores: Option<String>,
    pub threads: usize,
    pub total: f64,
    pub android: f64,
    pub ios: f64,
    pub flutter: f64,
}

struct Row {
    label: String,
    device: String,
    score: f64,
    n: usize,
    you: bool,
}

/// "13th Gen Intel(R) Core(TM) i7-13700H" -> "Intel Core i7-13700H",
/// "AMD Ryzen 7 5800X 8-Core Processor" -> "AMD Ryzen 7 5800X".
fn clean_cpu(cpu: &str) -> String {
    let cpu = cpu.split(" @ ").next().unwrap_or(cpu).split(" with ").next().unwrap_or(cpu).split(" w/ ").next().unwrap_or(cpu);
    let cpu = cpu.replace("(R)", "").replace("(r)", "").replace("(TM)", "").replace("(tm)", "");
    cpu.split_whitespace()
        .filter(|w| {
            let l = w.to_ascii_lowercase();
            !(l == "cpu" || l == "processor" || l == "gen" || l.ends_with("-core")
                || (l.ends_with("th") || l.ends_with("st") || l.ends_with("nd") || l.ends_with("rd"))
                    && l.len() <= 4 && l.chars().next().map_or(false, |c| c.is_ascii_digit()))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn hw_label(cpu: &str, cores: &str, threads: u32) -> String {
    let cpu = clean_cpu(cpu);
    if cores.is_empty() {
        format!("{} · {}T", cpu, threads)
    } else if cores.contains('P') {
        format!("{} · {}", cpu, cores)
    } else {
        format!("{} · {}C/{}T", cpu, cores, threads)
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

fn most_common(v: &[&'static str]) -> &'static str {
    let mut best = ("", 0);
    for &m in v.iter().filter(|m| !m.is_empty()) {
        let c = v.iter().filter(|&&x| x == m).count();
        if c > best.1 {
            best = (m, c);
        }
    }
    best.0
}

fn rows(score_version: u32, key: Key) -> Vec<Row> {
    let mut groups: Vec<(String, Vec<&Record>)> = Vec::new();
    for r in RECORDS.iter().filter(|r| r.score_version == score_version) {
        let label = hw_label(r.cpu, r.cores, r.threads);
        match groups.iter_mut().find(|(l, _)| *l == label) {
            Some((_, v)) => v.push(r),
            None => groups.push((label, vec![r])),
        }
    }
    groups
        .into_iter()
        .map(|(label, v)| Row {
            device: most_common(&v.iter().map(|r| r.model).collect::<Vec<_>>()).to_string(),
            score: median(v.iter().map(|r| key.of(r)).collect()),
            n: v.len(),
            label,
            you: false,
        })
        .collect()
}

fn fit(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w { format!("{}{}", s, " ".repeat(w - n)) } else { s.chars().take(w - 1).collect::<String>() + "…" }
}

/// Prints the ranking. Returns false when there is nothing to compare against.
pub fn print(score_version: u32, key: Key, mine: Option<&Mine>) -> bool {
    let mut list = rows(score_version, key);
    if list.is_empty() {
        return false;
    }
    let total_results: usize = list.iter().map(|r| r.n).sum();
    if let Some(m) = mine {
        list.push(Row {
            label: hw_label(&m.cpu, m.cores.as_deref().unwrap_or(""), m.threads as u32),
            device: "this machine".into(),
            score: match key {
                Key::Total => m.total,
                Key::Android => m.android,
                Key::Ios => m.ios,
                Key::Flutter => m.flutter,
            },
            n: 0,
            you: true,
        });
    }
    list.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

    // long lists: top 3, the neighbourhood of "you", and the last one
    let you = list.iter().position(|r| r.you);
    let show = |i: usize| list.len() <= 15 || i < 3 || i + 1 == list.len() || you.map_or(i < 12, |y| i + 4 >= y && i <= y + 4);

    let max = list.iter().map(|r| r.score).fold(1.0, f64::max);
    println!(
        "\n  How you compare — {} (median per CPU, {} result{})",
        key.title(),
        total_results,
        if total_results == 1 { "" } else { "s" }
    );
    println!("  {}", "─".repeat(86));
    let mut gap = false;
    for (i, r) in list.iter().enumerate() {
        if !show(i) {
            if !gap {
                println!("       ⋮");
                gap = true;
            }
            continue;
        }
        gap = false;
        let bar_len = (r.score / max * 20.0).round() as usize;
        println!(
            "  {} {:>3}  {} {} {:>6.0}  {}{}",
            if r.you { "▶" } else { " " },
            i + 1,
            fit(&r.label, 32),
            fit(&r.device, 24),
            r.score,
            fit(&"█".repeat(bar_len), 20),
            if r.you { "  ◀ you".to_string() } else { format!("  n={}", r.n) }
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI runs this on every pull request: a malformed or rule-breaking file in results/ fails the check.
    #[test]
    fn all_result_files_are_valid() {
        assert!(REJECTED.is_empty(), "invalid files in results/: {:#?}", REJECTED);
    }

    #[test]
    fn cpu_names_are_cleaned() {
        assert_eq!(clean_cpu("13th Gen Intel(R) Core(TM) i7-13700H"), "Intel Core i7-13700H");
        assert_eq!(clean_cpu("AMD Ryzen 7 5800X 8-Core Processor"), "AMD Ryzen 7 5800X");
        assert_eq!(clean_cpu("Intel(R) Core(TM) i5-8250U CPU @ 1.60GHz"), "Intel Core i5-8250U");
        assert_eq!(clean_cpu("AMD Ryzen 7 7840HS w/ Radeon 780M Graphics"), "AMD Ryzen 7 7840HS");
        assert_eq!(clean_cpu("Apple M4 Pro"), "Apple M4 Pro");
    }

    #[test]
    fn median_works() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(vec![4.0, 1.0, 2.0, 3.0]), 2.5);
    }
}
