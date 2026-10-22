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
    /// Merger / ExtractAudio / FixupM4a / etc. -> post-process phase.
    PostProcess(String),
    Ignore,
}

fn parse_size(s: &str) -> Option<i64> {
    let s = s.trim();
    if s == "UnknownB" || s == "NA" {
        return None;
    }
    let (num, unit) = s.split_at(s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len()));
    let num: f64 = num.trim().parse().ok()?;
    let mult = match unit.trim_end_matches('B').trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1.0,
        "K" | "KIB" => 1024.0,
        "M" | "MIB" => 1024f64.powi(2),
        "G" | "GIB" => 1024f64.powi(3),
        "T" | "TIB" => 1024f64.powi(4),
        _ => 1024.0, // yt-dlp uses KiB/MiB units
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

/// Parse one line of yt-dlp stdout (`--newline --progress`).
pub fn classify(line: &str) -> Line {
    let t = line.trim();
    if t.is_empty() {
        return Line::Ignore;
    }

    // Playlist item counter.
    if let Some(rest) = t.strip_prefix("[download] Downloading item ") {
        if let Some((idx, n)) = rest.split_once(" of ") {
            return match (idx.trim().parse(), n.trim().parse()) {
                (Ok(i), Ok(n)) => Line::Item { idx: i, n },
                _ => Line::Ignore,
            };
        }
    }

    if let Some(rest) = t.strip_prefix("[download] ") {
        if let Some(rest) = rest.strip_prefix("Destination: ") {
            let p = PathBuf::from(rest.trim());
            if p.as_os_str().len() > 0 {
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

    // Post-processing phases: [Merger], [ExtractAudio], [EmbedSubtitle]...
    if t.starts_with('[') {
        if let Some(end) = t.find(']') {
            let tag = &t[1..end];
            if tag == "download" {
                return Line::Ignore;
            }
            return Line::PostProcess(tag.to_string());
        }
    }

    // Bare absolute path (from `--print after_move:filepath`).
    let candidate = PathBuf::from(t);
    if candidate.as_os_str().len() > 0 && candidate.is_absolute() && !t.starts_with('[') {
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
    fn parses_progress_line() {
        let line = "[download]  42.3% of   12.34MiB at    2.15MiB/s ETA 00:03";
        match classify(line) {
            Line::Progress { pct, total, speed, eta, .. } => {
                assert!((pct - 42.3).abs() < 0.01);
                assert_eq!(total, Some(12_343_000));
                assert_eq!(speed, Some(2_154_000));
                assert_eq!(eta, Some(3));
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
