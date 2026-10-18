//! Best-effort hardware/OS description. Every probe is optional: on failure we print "unknown".

use std::process::Command;

pub struct SysInfo {
    /// e.g. "macOS 26.6.2 (build 25G84)", "Ubuntu 24.04.1 LTS (kernel 6.8.0)", "Windows 11 Pro (build 26100)"
    pub os: String,
    pub arch: &'static str,
    /// e.g. "MacBook Pro (Mac16,7)", "LENOVO ThinkPad X1 Carbon Gen 12"
    pub model: Option<String>,
    pub cpu: String,
    /// e.g. "10P + 4E" on hybrid CPUs, or physical core count
    pub cores: Option<String>,
    pub threads: usize,
    pub ram_gb: Option<f64>,
    pub on_battery: Option<bool>,
}

fn cmd(prog: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(prog).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Firmware placeholders that vendors leave in DMI fields.
fn meaningful(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    !["to be filled", "default string", "system product name", "system manufacturer", "not applicable", "none"]
        .iter()
        .any(|junk| l.contains(junk))
}

pub fn collect() -> SysInfo {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let mut info = SysInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH,
        model: None,
        cpu: "unknown CPU".into(),
        cores: None,
        threads,
        ram_gb: None,
        on_battery: None,
    };
    let mut ram_bytes: Option<f64> = None;

    match std::env::consts::OS {
        "macos" => {
            let name = cmd("sw_vers", &["-productName"]).unwrap_or_else(|| "macOS".into());
            let ver = cmd("sw_vers", &["-productVersion"]).unwrap_or_default();
            info.os = match cmd("sw_vers", &["-buildVersion"]) {
                Some(b) => format!("{} {} (build {})", name, ver, b),
                None => format!("{} {}", name, ver),
            };
            let ident = cmd("sysctl", &["-n", "hw.model"]);
            let marketing = cmd("system_profiler", &["SPHardwareDataType"]).and_then(|s| {
                s.lines().find_map(|l| l.trim().strip_prefix("Model Name:").map(|v| v.trim().to_string()))
            });
            info.model = match (marketing, ident) {
                (Some(m), Some(i)) => Some(format!("{} ({})", m, i)),
                (m, i) => m.or(i),
            };
            if let Some(c) = cmd("sysctl", &["-n", "machdep.cpu.brand_string"]) {
                info.cpu = c;
            }
            let p = cmd("sysctl", &["-n", "hw.perflevel0.physicalcpu"]);
            let e = cmd("sysctl", &["-n", "hw.perflevel1.physicalcpu"]);
            info.cores = match (p, e) {
                (Some(p), Some(e)) => Some(format!("{}P + {}E", p, e)),
                _ => cmd("sysctl", &["-n", "hw.physicalcpu"]),
            };
            ram_bytes = cmd("sysctl", &["-n", "hw.memsize"]).and_then(|s| s.parse().ok());
            info.on_battery = cmd("pmset", &["-g", "batt"]).map(|s| s.contains("'Battery Power'"));
        }
        "linux" => {
            let distro = read("/etc/os-release")
                .and_then(|s| s.lines().find(|l| l.starts_with("PRETTY_NAME=")).map(|l| l[12..].trim_matches('"').to_string()))
                .unwrap_or_else(|| "Linux".into());
            info.os = match cmd("uname", &["-r"]) {
                Some(k) => format!("{} (kernel {})", distro, k),
                None => distro,
            };
            let vendor = read("/sys/class/dmi/id/sys_vendor").filter(|s| meaningful(s));
            let product = read("/sys/class/dmi/id/product_version")
                .filter(|s| meaningful(s) && s.chars().any(|c| c.is_alphabetic()) && s.len() > 3)
                .or_else(|| read("/sys/class/dmi/id/product_name").filter(|s| meaningful(s)))
                .or_else(|| read("/proc/device-tree/model").map(|s| s.trim_end_matches('\0').to_string()));
            info.model = match (vendor, product) {
                (Some(v), Some(p)) if !p.starts_with(&v) => Some(format!("{} {}", v, p)),
                (v, p) => p.or(v),
            };
            let cpuinfo = read("/proc/cpuinfo").unwrap_or_default();
            if let Some(c) = cpuinfo
                .lines()
                .find(|l| l.starts_with("model name") || l.starts_with("Hardware") || l.starts_with("Model"))
                .and_then(|l| l.split(':').nth(1))
            {
                info.cpu = c.trim().to_string();
            }
            let mut physical = std::collections::HashSet::new();
            let mut pkg = "";
            for l in cpuinfo.lines() {
                if let Some(v) = l.strip_prefix("physical id") {
                    pkg = v;
                } else if let Some(v) = l.strip_prefix("core id") {
                    physical.insert((pkg, v));
                }
            }
            if !physical.is_empty() {
                info.cores = Some(physical.len().to_string());
            }
            ram_bytes = read("/proc/meminfo").and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<f64>().ok())
                    .map(|kb| kb * 1024.0)
            });
            info.on_battery = std::fs::read_dir("/sys/class/power_supply").ok().and_then(|dir| {
                let mut ac_seen = None;
                for e in dir.flatten() {
                    let p = e.path();
                    if read(&p.join("type").to_string_lossy()).map(|t| t == "Mains").unwrap_or(false) {
                        let online = read(&p.join("online").to_string_lossy()).map(|v| v == "1");
                        ac_seen = Some(ac_seen.unwrap_or(false) || online.unwrap_or(false));
                    }
                }
                ac_seen.map(|ac| !ac)
            });
        }
        "windows" => {
            // one PowerShell call instead of five: each start costs ~0.3 s
            let script = "$o=Get-CimInstance Win32_OperatingSystem; $c=Get-CimInstance Win32_ComputerSystem; \
                $p=Get-CimInstance Win32_Processor | Select-Object -First 1; \
                $b=Get-CimInstance Win32_Battery | Select-Object -First 1; \
                $d=(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion' -ErrorAction SilentlyContinue).DisplayVersion; \
                \"os=$($o.Caption)\"; \"ver=$d\"; \"build=$($o.BuildNumber)\"; \"vendor=$($c.Manufacturer)\"; \"model=$($c.Model)\"; \
                \"cpu=$($p.Name)\"; \"cores=$($p.NumberOfCores)\"; \"ram=$($c.TotalPhysicalMemory)\"; \"batt=$($b.BatteryStatus)\"";
            let out = cmd("powershell", &["-NoProfile", "-Command", script]).unwrap_or_default();
            let get = |k: &str| {
                out.lines()
                    .find_map(|l| l.strip_prefix(k).and_then(|v| v.strip_prefix('=')))
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            };
            let caption = get("os").unwrap_or_else(|| "Windows".into());
            let caption = caption.strip_prefix("Microsoft ").unwrap_or(&caption).to_string();
            info.os = match (get("ver"), get("build")) {
                (Some(v), Some(b)) => format!("{} {} (build {})", caption, v, b),
                (None, Some(b)) => format!("{} (build {})", caption, b),
                _ => caption,
            };
            let vendor = get("vendor").filter(|s| meaningful(s));
            let model = get("model").filter(|s| meaningful(s));
            info.model = match (vendor, model) {
                (Some(v), Some(m)) if !m.starts_with(&v) => Some(format!("{} {}", v, m)),
                (v, m) => m.or(v),
            };
            if let Some(c) = get("cpu").or_else(|| std::env::var("PROCESSOR_IDENTIFIER").ok()) {
                info.cpu = c;
            }
            info.cores = get("cores");
            ram_bytes = get("ram").and_then(|s| s.parse().ok());
            info.on_battery = get("batt").and_then(|s| s.parse::<u32>().ok()).map(|s| s == 1); // 1 = discharging
        }
        _ => {}
    }
    info.ram_gb = ram_bytes.map(|b| b / (1u64 << 30) as f64);
    info
}
