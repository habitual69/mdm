//! yt-dlp line classes: progress, item counters, merge phases, destination
//! paths and `--print after_move:filepath` bare lines.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Progress {
        pct: f32,
        downloaded: Option<i64>,
        total: Option<i64>,
        speed: Option<i64>,
        eta: Option<i64>,
    },
    Item { idx: u64, n: u64 },
    Destination(PathBuf),
    FinalPath(PathBuf),
    /// Video/playlist title from `--print "mdm_title:%(title)s"`.
    Title(String),
    /// Merger / ExtractAudio / FixupM4a / etc. -> post-process phase.
    PostProcess(String),
    Ignore,
}

fn parse_size(s: &str) -> Option<i64> {
    let s = s.trim();
    if s == "UnknownB" || s == "NA" || s.is_empty() {
        return None;
    }
    let (num, unit) = s.split_at(s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len()));
    let num: f64 = num.trim().parse().ok()?;
    let mult = match unit.to_ascii_uppercase().as_str() {
        "" | "B" => 1.0,
        "KB" | "KIB" => 1024.0,
        "MB" | "MIB" => 1024f64.powi(2),
        "GB" | "GIB" => 1024f64.powi(3),
        "TB" | "TIB" => 1024f64.powi(4),
        _ => return None,
    };
    Some((num * mult) as i64)
}

fn parse_eta(s: &str) -> Option<i64> {
    // "ETA 00:03" | "ETA 01:02:03" | "ETA Unknown"
    if s.contains("Unknown") {
        return None;
    }
    let parts: Vec<&str> = s.trim().split(':').collect();
    match parts.as_slice() {
        [mm, ss] => Some(mm.parse::<i64>().ok()? * 60 + ss.parse::<i64>().ok()?),
        [hh, mm, ss] => {
            Some(hh.parse::<i64>().ok()? * 3600 + mm.parse::<i64>().ok()? * 60 + ss.parse::<i64>().ok()?)
        }
        _ => None,
    }
}

fn parse_aria_eta(s: &str) -> Option<i64> {
    if s.is_empty() || s.contains("Unknown") {
        return None;
    }
    if s.contains(':') {
        return parse_eta(s);
    }
    let mut total_sec = 0i64;
    let mut num_buf = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num_buf.push(c);
        } else if !num_buf.is_empty() {
            let val: i64 = num_buf.parse().ok()?;
            num_buf.clear();
            match c {
                's' | 'S' => total_sec += val,
                'm' | 'M' => total_sec += val * 60,
                'h' | 'H' => total_sec += val * 3600,
                'd' | 'D' => total_sec += val * 86400,
                _ => {}
            }
        }
    }
    if !num_buf.is_empty() && total_sec == 0 {
        total_sec = num_buf.parse().ok()?;
    }
    Some(total_sec)
}

/// Parse one line of yt-dlp stdout (`--newline --progress`).
pub fn classify(line: &str) -> Line {
    let t = line.trim();
    if t.is_empty() {
        return Line::Ignore;
    }

    // Early title line from `--print "mdm_title:%(title)s"`.
    if let Some(title) = t.strip_prefix("mdm_title:") {
        let title = title.trim();
        if title.is_empty() {
            return Line::Ignore;
        }
        return Line::Title(title.to_string());
    }

    // Playlist item counter.
    if let Some(rest) = t.strip_prefix("[download] Downloading item ")
        && let Some((idx, n)) = rest.split_once(" of ")
    {
        return match (idx.trim().parse(), n.trim().parse()) {
            (Ok(i), Ok(n)) => Line::Item { idx: i, n },
            _ => Line::Ignore,
        };
    }

    if let Some(rest) = t.strip_prefix("[download] ") {
        if let Some(rest) = rest.strip_prefix("Destination: ") {
            let p = PathBuf::from(rest.trim());
            if !p.as_os_str().is_empty() {
                return Line::Destination(p);
            }
        }
        let body = rest;
        let pct = body
            .split_whitespace()
            .next()
            .and_then(|tok| tok.strip_suffix('%'))
            .and_then(|p| p.parse::<f32>().ok());
        if let Some(pct) = pct {
            let mut it = body.split_whitespace().skip(1);
            let mut total = None;
            let mut speed = None;
            let mut eta = None;
            while let Some(tok) = it.next() {
                if tok == "of" {
                    if let Some(t) = it.next() {
                        total = parse_size(t);
                    }
                } else if tok == "at" {
                    if let Some(s) = it.next() {
                        // "2.15MiB/s" or "Unknown B/s"
                        speed = parse_size(s.trim_end_matches("/s"));
                    }
                    break;
                }
            }
            if let Some(pos) = body.find("ETA") {
                eta = parse_eta(body[pos + 3..].trim());
            }
            return Line::Progress { pct, downloaded: None, total, speed, eta };
        }
    }

    // aria2c progress format: [#2089b0 3.2MiB/14MiB(22%) CN:4 DL:2.5MiB ETA:4s]
    if t.starts_with("[#") {
        let mut pct = None;
        let mut downloaded = None;
        let mut total = None;
        let mut speed = None;
        let mut eta = None;

        if let Some(open) = t.find('(') && let Some(close) = t[open..].find("%)") {
            let pct_str = &t[open + 1..open + close];
            pct = pct_str.parse::<f32>().ok();
        }

        if let Some(slash) = t.find('/') {
            let start = t[..slash].rfind(' ').map(|i| i + 1).unwrap_or(0);
            let end = t[slash..].find('(').map(|i| slash + i).unwrap_or(t.len());
            let d_str = t[start..slash].trim();
            let t_str = t[slash + 1..end].trim();
            downloaded = parse_size(d_str);
            total = parse_size(t_str);
        }

        if let Some(pos) = t.find("DL:") {
            let rest = &t[pos + 3..];
            let end = rest.find([' ', ']', '\r', '\n']).unwrap_or(rest.len());
            let s_str = rest[..end].trim().trim_end_matches("/s");
            speed = parse_size(s_str);
        }

        if let Some(pos) = t.find("ETA:") {
            let rest = &t[pos + 4..];
            let end = rest.find([' ', ']', '\r', '\n']).unwrap_or(rest.len());
            let eta_str = rest[..end].trim();
            eta = parse_aria_eta(eta_str);
        }

        if let Some(pct) = pct {
            return Line::Progress { pct, downloaded, total, speed, eta };
        }
    }

    // Post-processing phases: [Merger], [ExtractAudio], [EmbedSubtitle]...
    if t.starts_with('[') && let Some(end) = t.find(']') {
        let tag = &t[1..end];
        if tag == "download" {
            return Line::Ignore;
        }
        return Line::PostProcess(tag.to_string());
    }

    // Bare absolute path (from `--print after_move:filepath`).
    let candidate = PathBuf::from(t);
    if !candidate.as_os_str().is_empty() && candidate.is_absolute() && !t.starts_with('[') {
        return Line::FinalPath(candidate);
    }

    Line::Ignore
}

#[allow(dead_code)]
pub fn is_final_path(line: &str) -> Option<PathBuf> {
    match classify(line) {
        Line::FinalPath(p) => Some(p),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_line() {
        assert_eq!(classify("mdm_title:Hello World"), Line::Title("Hello World".into()));
        assert_eq!(classify("mdm_title:  "), Line::Ignore);
        assert_eq!(classify("[download] Destination: x"), Line::Destination("x".into()));
    }

    #[test]
    fn parses_progress_line() {
        let line = "[download]  42.3% of   12.34MiB at    2.15MiB/s ETA 00:03";
        match classify(line) {
            Line::Progress { pct, total, speed, eta, .. } => {
                assert!((pct - 42.3).abs() < 0.01);
                assert_eq!(total, Some((12.34f64 * 1024.0 * 1024.0) as i64));
                assert_eq!(speed, Some((2.15f64 * 1024.0 * 1024.0) as i64));
                assert_eq!(eta, Some(3));
            }
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn parses_aria2_progress_line() {
        let line = "[#2089b0 3.2MiB/14.0MiB(22%) CN:4 DL:2.5MiB ETA:4s]";
        match classify(line) {
            Line::Progress { pct, total, speed, eta, .. } => {
                assert_eq!(pct, 22.0);
                assert_eq!(total, Some((14.0f64 * 1024.0 * 1024.0) as i64));
                assert_eq!(speed, Some((2.5f64 * 1024.0 * 1024.0) as i64));
                assert_eq!(eta, Some(4));
            }
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn parses_playlist_item() {
        assert_eq!(
            classify("[download] Downloading item 2 of 5"),
            Line::Item { idx: 2, n: 5 }
        );
    }

    #[test]
    fn parses_destination() {
        assert_eq!(
            classify("[download] Destination: /tmp/out.mp4"),
            Line::Destination(PathBuf::from("/tmp/out.mp4"))
        );
    }

    #[test]
    fn parses_merger_as_postprocess() {
        match classify("[Merger] Merging formats into \"/tmp/x.webm\"") {
            Line::PostProcess(tag) => assert_eq!(tag, "Merger"),
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn ignores_garbage() {
        assert_eq!(classify(""), Line::Ignore);
        assert_eq!(classify("random text"), Line::Ignore);
    }

    #[test]
    fn handles_hh_eta() {
        let line = "[download]  10.0% of    1.00GiB at  100.00KiB/s ETA 01:02:03";
        match classify(line) {
            Line::Progress { eta, .. } => assert_eq!(eta, Some(3723)),
            other => panic!("wrong: {other:?}"),
        }
    }
}
