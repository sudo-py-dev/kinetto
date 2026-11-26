//! Times as the editor shows them. Every display rounds once and then splits
//! into minutes and seconds, so 59.6 s reads `1:00`, never `0:00` or `0:60`.

/// `m:ss`, to the nearest second: lengths and the playhead under the slide.
pub fn whole(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// `m:ss.s`, to the nearest tenth: the timeline, where precision matters.
pub fn tenths(secs: f64) -> String {
    let t = (secs.max(0.0) * 10.0).round() as u64;
    format!("{}:{:02}.{}", t / 600, t % 600 / 10, t % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_rounds_before_splitting() {
        assert_eq!(whole(0.0), "0:00");
        assert_eq!(whole(21.4), "0:21");
        assert_eq!(whole(59.6), "1:00");
        assert_eq!(whole(179.6), "3:00");
        assert_eq!(whole(-0.2), "0:00");
    }

    #[test]
    fn tenths_rounds_before_splitting() {
        assert_eq!(tenths(65.24), "1:05.2");
        assert_eq!(tenths(59.96), "1:00.0");
        assert_eq!(tenths(0.04), "0:00.0");
        assert_eq!(tenths(600.0), "10:00.0");
    }
}
