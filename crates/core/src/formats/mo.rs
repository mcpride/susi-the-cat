//! Binary GNU gettext MO files, compiled from (localized) PO files like `msgfmt`.
//!
//! Layout: magic `0x950412de`, revision 0, message count, offsets of the sorted original and
//! translation tables, and an empty hash table (readers fall back to binary search). Contexts are
//! encoded as `msgctxt\x04msgid`, plural messages as `msgid\0msgid_plural` with the forms joined
//! by `\0`. Fuzzy messages are skipped (as `msgfmt` does).

use super::po;
use crate::error::{Error, Result};

const MAGIC: u32 = 0x9504_12de;
const HEADER_SIZE: usize = 28;

/// Compiles PO text to MO bytes. Untranslated messages are written only with `include_empty`.
pub fn compile(po_bytes: &[u8], include_empty: bool, big_endian: bool) -> Result<Vec<u8>> {
    let entries = po::parse(po_bytes)?;
    let mut msgs: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for e in entries.iter().filter(|e| e.is_message() && !e.is_fuzzy()) {
        let mut key = String::new();
        if let Some(c) = e.context() {
            key.push_str(&c);
            key.push('\u{4}');
        }
        key.push_str(&e.id().unwrap_or_default());
        let forms: Vec<String> = match e.plural() {
            Some(p) => {
                key.push('\0');
                key.push_str(&p);
                let n = e.msgstr.iter().map(|(i, _)| i.unwrap_or(0) + 1).max().unwrap_or(1);
                (0..n).map(|i| e.msgstr(i)).collect()
            }
            None => vec![e.msgstr(0)],
        };
        if !e.is_header() && !include_empty && forms.iter().any(String::is_empty) {
            continue;
        }
        msgs.push((key.into_bytes(), forms.join("\0").into_bytes()));
    }
    msgs.sort_by(|a, b| a.0.cmp(&b.0));
    msgs.dedup_by(|a, b| a.0 == b.0);
    write(&msgs, big_endian)
}

fn to_u32(v: usize) -> Result<u32> {
    u32::try_from(v).map_err(|_| Error::Format("MO file too large".into()))
}

fn write(msgs: &[(Vec<u8>, Vec<u8>)], big_endian: bool) -> Result<Vec<u8>> {
    let put = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
    let n = msgs.len();
    let orig_table = HEADER_SIZE;
    let trans_table = orig_table + 8 * n;
    let hash_table = trans_table + 8 * n;
    let mut out = Vec::new();
    for v in [MAGIC, 0, to_u32(n)?, to_u32(orig_table)?, to_u32(trans_table)?, 0, to_u32(hash_table)?] {
        put(&mut out, v);
    }
    let mut strings = Vec::new();
    let mut tables = [Vec::new(), Vec::new()];
    for (slot, pick) in tables.iter_mut().zip([0usize, 1]) {
        for (k, v) in msgs {
            let s = if pick == 0 { k } else { v };
            put(slot, to_u32(s.len())?);
            put(slot, to_u32(hash_table + strings.len())?);
            strings.extend_from_slice(s);
            strings.push(0);
        }
    }
    for t in &tables {
        out.extend_from_slice(t);
    }
    out.extend_from_slice(&strings);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal MO reader for the tests.
    fn read(mo: &[u8]) -> Vec<(String, String)> {
        let le = mo[..4] == MAGIC.to_le_bytes();
        let u = |o: usize| {
            let b = [mo[o], mo[o + 1], mo[o + 2], mo[o + 3]];
            (if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }) as usize
        };
        assert_eq!(u(0), MAGIC as usize);
        let (n, ot, tt) = (u(8), u(12), u(16));
        let s = |table: usize, i: usize| {
            let (len, off) = (u(table + 8 * i), u(table + 8 * i + 4));
            assert_eq!(mo[off + len], 0);
            String::from_utf8(mo[off..off + len].to_vec()).unwrap()
        };
        (0..n).map(|i| (s(ot, i), s(tt, i))).collect()
    }

    const PO: &str = "msgid \"\"\nmsgstr \"Content-Type: text/plain; charset=UTF-8\\n\"\n\nmsgid \"b\"\nmsgstr \"B\"\n\nmsgctxt \"m\"\nmsgid \"a\"\nmsgstr \"A\"\n\nmsgid \"empty\"\nmsgstr \"\"\n\n#, fuzzy\nmsgid \"fz\"\nmsgstr \"FZ\"\n\nmsgid \"file\"\nmsgid_plural \"files\"\nmsgstr[0] \"Datei\"\nmsgstr[1] \"Dateien\"\n";

    #[test]
    fn compiles_sorted_messages() {
        let m = read(&compile(PO.as_bytes(), false, false).unwrap());
        let keys: Vec<&str> = m.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["", "b", "file\0files", "m\u{4}a"]);
        assert_eq!(m[0].1, "Content-Type: text/plain; charset=UTF-8\n");
        assert_eq!(m[2].1, "Datei\0Dateien");
    }

    #[test]
    fn include_empty_and_big_endian() {
        let mo = compile(PO.as_bytes(), true, true).unwrap();
        assert_eq!(mo[..4], MAGIC.to_be_bytes());
        let m = read(&mo);
        assert!(m.iter().any(|(k, v)| k == "empty" && v.is_empty()));
        assert!(!m.iter().any(|(k, _)| k == "fz"));
    }
}
