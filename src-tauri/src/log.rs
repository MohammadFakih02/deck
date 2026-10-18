//! Deck's own log (`%LOCALAPPDATA%\Deck\logs\deck.log`): what went wrong in the background, where
//! nobody sees a console. Kept small (a few files of 1 MB); its end goes into the support report.

use parking_lot::Mutex;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub struct Log {
    dir: PathBuf,
    max: u64,
    keep: usize,
    lock: Mutex<()>,
}

impl Log {
    /// A log in `dir`: a new file after `max` bytes, `keep` older files.
    pub fn new(dir: &Path, max: u64, keep: usize) -> Log {
        Log { dir: dir.to_path_buf(), max, keep, lock: Mutex::new(()) }
    }

    fn file(&self, i: usize) -> PathBuf {
        self.dir.join(if i == 0 { "deck.log".to_string() } else { format!("deck.{i}.log") })
    }

    pub fn write(&self, level: &str, msg: &str) {
        let line = format!("{} [{level}] {}\r\n", stamp(crate::util::now_s()), msg.replace(['\r', '\n'], " | "));
        let _g = self.lock.lock();
        let _ = std::fs::create_dir_all(&self.dir);
        let now = std::fs::metadata(self.file(0)).map(|m| m.len()).unwrap_or(0);
        if now > 0 && now + line.len() as u64 > self.max {
            let _ = std::fs::remove_file(self.file(self.keep));
            for i in (0..self.keep).rev() {
                let _ = std::fs::rename(self.file(i), self.file(i + 1));
            }
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.file(0)) {
            let _ = f.write_all(line.as_bytes());
        }
    }

    /// The last `lines` lines (older files too, when the current one is shorter).
    pub fn tail(&self, lines: usize) -> String {
        let _g = self.lock.lock();
        let mut out: Vec<String> = Vec::new();
        for i in 0..=self.keep {
            let Ok(t) = std::fs::read_to_string(self.file(i)) else { break };
            let mut older: Vec<String> = t.lines().map(String::from).collect();
            older.append(&mut out);
            out = older;
            if out.len() >= lines {
                break;
            }
        }
        let skip = out.len().saturating_sub(lines);
        out[skip..].join("\r\n")
    }
}

/// "2026-10-17 14:03:09Z" (UTC) for unix seconds.
fn stamp(s: u64) -> String {
    let days = (s / 86400) as i64;
    // civil date from days since 1970 (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let t = s % 86400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

/// Where the log lives: `%LOCALAPPDATA%\Deck\logs` (`Deck-<profile>` for a test profile).
pub fn dir() -> PathBuf {
    // the tests' warnings stay out of the real log
    if cfg!(test) {
        return std::env::temp_dir().join("deck-test-logs");
    }
    let name = match std::env::var("DECK_PROFILE") {
        Ok(p) if !p.is_empty() => format!("Deck-{p}"),
        _ => "Deck".to_string(),
    };
    crate::util::local_appdata().join(name).join("logs")
}

fn global() -> &'static Log {
    static LOG: OnceLock<Log> = OnceLock::new();
    LOG.get_or_init(|| Log::new(&dir(), 1 << 20, 3))
}

/// Something went wrong that Deck carried on from.
pub fn warn(msg: impl AsRef<str>) {
    global().write("warn", msg.as_ref());
}

/// Something worth knowing later (a system change Deck made).
pub fn info(msg: impl AsRef<str>) {
    global().write("info", msg.as_ref());
}

/// The log's last `lines` lines.
pub fn tail(lines: usize) -> String {
    global().tail(lines)
}

#[cfg(test)]
mod tests {
    use super::Log;

    #[test]
    fn lines_are_kept_in_order_and_old_files_rotate_out() {
        let dir = std::env::temp_dir().join(format!("deck-log-{}", crate::util::now_ms()));
        let log = Log::new(&dir, 400, 2);
        for i in 0..100 {
            log.write("warn", &format!("line {i:03}"));
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["deck.1.log", "deck.2.log", "deck.log"], "the current file and two older ones");
        let size: u64 = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.metadata().unwrap().len()).sum();
        assert!(size < 3 * 500, "small: {size} bytes");
        let t = log.tail(15);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 15);
        assert!(lines.last().unwrap().ends_with("[warn] line 099"), "{t}");
        assert!(lines[0].ends_with("line 085"), "across the rotation, in order: {t}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(super::stamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(super::stamp(1_792_259_589), "2026-10-17 17:53:09Z");
    }
}
