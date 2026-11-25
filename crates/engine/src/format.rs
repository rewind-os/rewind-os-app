//! Display helpers shared by the front ends.

/// Size in binary units with the labels Windows uses (1 KB = 1024 bytes).
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// Thousands separated with a thin space, readable in any language.
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202F}');
        }
        out.push(c);
    }
    out
}

/// A change in size with its sign: "+1.2 GB", "-300 MB" (an ASCII minus: every font has it).
pub fn signed_bytes(delta: i128) -> String {
    let size = bytes(u64::try_from(delta.unsigned_abs()).unwrap_or(u64::MAX));
    if delta < 0 {
        format!("-{size}")
    } else if delta == 0 {
        size
    } else {
        format!("+{size}")
    }
}

/// A moment as a date in this computer's time zone: "2026-11-25".
pub fn local_date(at: chrono::DateTime<chrono::Utc>) -> String {
    at.with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string()
}

/// A duration for people: `850 ms`, `4.2 s`, `1 min 05 s`.
pub fn duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        let s = (ms + 500) / 1000;
        format!("{} min {:02} s", s / 60, s % 60)
    }
}
