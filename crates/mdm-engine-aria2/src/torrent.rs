//! Minimal bencode reader for `.torrent` previews (name, file list, sizes).

use std::path::Path;

use mdm_core::{Error, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TorrentInfo {
    pub name: String,
    pub total: u64,
    pub files: Vec<TorrentFile>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TorrentFile {
    /// Relative path inside the torrent (may be a single segment).
    pub name: String,
    pub size: u64,
}

enum Value<'a> {
    Int(u64),
    Bytes(&'a [u8]),
    List(Vec<Value<'a>>),
    Dict(Vec<(&'a [u8], Value<'a>)>),
}

impl Value<'_> {
    fn get(&self, key: &[u8]) -> Option<&Value<'_>> {
        match self {
            Value::Dict(pairs) => pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<String> {
        match self {
            Value::Bytes(b) => String::from_utf8(b.to_vec()).ok(),
            _ => None,
        }
    }

    fn as_int(&self) -> Option<u64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }
}

fn parse<'a>(buf: &'a [u8], pos: &mut usize, depth: u8) -> Option<Value<'a>> {
    if depth > 64 || *pos >= buf.len() {
        return None;
    }
    let start = buf[*pos];
    let out = match start {
        b'i' => {
            let end = buf[*pos..].iter().position(|&b| b == b'e')? + *pos;
            let digits = std::str::from_utf8(&buf[*pos + 1..end]).ok()?;
            let n = digits.parse::<u64>().ok()?;
            *pos = end + 1;
            Value::Int(n)
        }
        b'0'..=b'9' => {
            let colon = buf[*pos..].iter().position(|&b| b == b':')? + *pos;
            let len = std::str::from_utf8(&buf[*pos..colon]).ok()?.parse::<usize>().ok()?;
            let from = colon + 1;
            if buf.len() < from + len {
                return None;
            }
            let slice = &buf[from..from + len];
            *pos = from + len;
            Value::Bytes(slice)
        }
        b'l' => {
            *pos += 1;
            let mut items = Vec::new();
            while buf.get(*pos).copied() != Some(b'e') {
                items.push(parse(buf, pos, depth + 1)?);
            }
            *pos += 1;
            Value::List(items)
        }
        b'd' => {
            *pos += 1;
            let mut pairs = Vec::new();
            while buf.get(*pos).copied() != Some(b'e') {
                let k = match parse(buf, pos, depth + 1)? {
                    Value::Bytes(b) => b,
                    _ => return None,
                };
                let v = parse(buf, pos, depth + 1)?;
                pairs.push((k, v));
            }
            *pos += 1;
            Value::Dict(pairs)
        }
        _ => return None,
    };
    Some(out)
}

/// Reads a .torrent file and returns its display name + per-file sizes.
/// aria2's `select-file` indices equal the 1-based position in `files`.
pub fn torrent_info(path: &Path) -> Result<TorrentInfo> {
    let bytes =
        std::fs::read(path).map_err(|e| Error::Config(format!("read torrent: {e}")))?;
    let mut pos = 0usize;
    let top = parse(&bytes, &mut pos, 0)
        .ok_or_else(|| Error::Config("invalid bencode".into()))?;
    let info = top
        .get(b"info")
        .ok_or_else(|| Error::Config("no info dict".into()))?;

    let name = info
        .get(b"name")
        .and_then(Value::as_str)
        .unwrap_or_else(|| "torrent".into());

    let mut files: Vec<TorrentFile> = Vec::new();
    if let Some(Value::List(list_ref)) = info.get(b"files") {
        for f in list_ref {
            let path_str = match f.get(b"path") {
                Some(Value::List(parts)) => Some(
                    parts
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("/"),
                ),
                _ => None,
            };
            let size = f.get(b"length").and_then(Value::as_int).unwrap_or(0);
            files.push(TorrentFile {
                name: path_str.unwrap_or_default(),
                size,
            });
        }
    } else if let Some(total) = info.get(b"length").and_then(Value::as_int) {
        files.push(TorrentFile { name: name.clone(), size: total });
    }

    let total = info
        .get(b"length")
        .and_then(Value::as_int)
        .unwrap_or_else(|| files.iter().map(|f| f.size).sum());
    Ok(TorrentInfo { name, total, files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_file_torrent() {
        // d4:infod10:created by2:xx4:namexi2e... minimal:
        // {"info":{"name":"me.iso","length":100}}
        let body = b"d4:infod4:name6:me.iso6:lengthi100eee";
        let mut pos = 0;
        let v = parse(body, &mut pos, 0).unwrap();
        let name = v.get(b"info").unwrap().get(b"name").unwrap().as_str().unwrap();
        assert_eq!(name, "me.iso");
    }
}
