//! Mise à jour incrémentale (ISO 32000-1, §7.5.6) : le fichier d'origine est recopié
//! tel quel, puis suivi des seuls objets nouveaux ou modifiés et d'une nouvelle
//! section de références croisées chaînée à la précédente par `/Prev`.

use std::io::{self, Write};

use lopdf::xref::XrefType;
use lopdf::{Dictionary, Object, ObjectId, Stream, Writer};

use crate::error::Result;
use crate::output::{AtomicOutput, CountingWriter};
use crate::progress::{CancelToken, Progress, ProgressSink};

const COPY_CHUNK: usize = 8 << 20;

/// Ce qu'il faut savoir du document d'origine pour lui ajouter une révision.
#[derive(Debug, Clone)]
pub(super) struct PreviousRevision<'a> {
    pub bytes: &'a [u8],
    /// Position de la dernière section de références croisées.
    pub xref_start: u64,
    pub xref_type: XrefType,
    /// Dictionnaire de fin (`trailer`) de la dernière révision.
    pub trailer: &'a Dictionary,
}

pub(super) fn write(
    output: &mut AtomicOutput,
    previous: &PreviousRevision<'_>,
    mut objects: Vec<(ObjectId, Object)>,
    next_free_id: u32,
    progress: &dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<()> {
    let total = previous.bytes.len() as u64;
    for chunk in previous.bytes.chunks(COPY_CHUNK) {
        cancel.check()?;
        let out = output.writer();
        out.write_all(chunk).map_err(|e| output.write_error(e))?;
        progress.report(Progress::Copying {
            done: output.writer().position(),
            total,
        });
    }
    append(output.writer(), previous, &mut objects, next_free_id).map_err(|e| output.write_error(e))
}

fn append<W: Write>(
    out: &mut CountingWriter<W>,
    previous: &PreviousRevision<'_>,
    objects: &mut [(ObjectId, Object)],
    next_free_id: u32,
) -> io::Result<()> {
    if !matches!(previous.bytes.last(), Some(b'\n' | b'\r')) {
        out.write_all(b"\n")?;
    }

    objects.sort_by_key(|(id, _)| *id);
    let mut entries = Vec::with_capacity(objects.len() + 1);
    for (id, object) in objects.iter() {
        entries.push(Entry {
            id: id.0,
            generation: id.1,
            offset: out.position(),
        });
        write_indirect_object(out, *id, object)?;
    }

    let mut trailer = Dictionary::new();
    for key in [&b"Root"[..], b"Info", b"ID"] {
        if let Ok(value) = previous.trailer.get(key) {
            trailer.set(key, value.clone());
        }
    }
    trailer.set("Prev", Object::Integer(previous.xref_start as i64));

    match previous.xref_type {
        XrefType::CrossReferenceTable => write_table(out, &entries, trailer, next_free_id),
        XrefType::CrossReferenceStream => write_stream(out, entries, trailer, next_free_id),
    }
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    id: u32,
    generation: u16,
    offset: u64,
}

pub(super) fn write_indirect_object(out: &mut impl Write, id: ObjectId, object: &Object) -> io::Result<()> {
    writeln!(out, "{} {} obj", id.0, id.1)?;
    Writer::write_object(out, object)?;
    out.write_all(b"\nendobj\n")
}

/// Regroupe les entrées en sous-sections d'identifiants consécutifs.
fn subsections(entries: &[Entry]) -> Vec<&[Entry]> {
    let mut groups = Vec::new();
    let mut start = 0;
    for i in 1..=entries.len() {
        if i == entries.len() || entries[i].id != entries[i - 1].id + 1 {
            groups.push(&entries[start..i]);
            start = i;
        }
    }
    groups
}

fn write_table<W: Write>(
    out: &mut CountingWriter<W>,
    entries: &[Entry],
    mut trailer: Dictionary,
    size: u32,
) -> io::Result<()> {
    let xref_start = out.position();
    out.write_all(b"xref\n")?;
    for group in subsections(entries) {
        writeln!(out, "{} {}", group[0].id, group.len())?;
        for entry in group {
            // Chaque entrée fait exactement 20 octets, fin de ligne comprise.
            write!(out, "{:010} {:05} n\r\n", entry.offset, entry.generation)?;
        }
    }
    trailer.set("Size", Object::Integer(i64::from(size)));
    out.write_all(b"trailer\n")?;
    Writer::write_object(out, &Object::Dictionary(trailer))?;
    write!(out, "\nstartxref\n{xref_start}\n%%EOF\n")
}

fn write_stream<W: Write>(
    out: &mut CountingWriter<W>,
    mut entries: Vec<Entry>,
    mut trailer: Dictionary,
    xref_id: u32,
) -> io::Result<()> {
    let xref_start = out.position();
    // La section se décrit elle-même.
    entries.push(Entry {
        id: xref_id,
        generation: 0,
        offset: xref_start,
    });

    let offset_width = (1..=8u8)
        .find(|bytes| *bytes == 8 || xref_start < 1u64 << (8 * bytes))
        .expect("8 octets suffisent toujours");
    let mut data = Vec::with_capacity(entries.len() * (3 + usize::from(offset_width)));
    let mut index = Vec::new();
    for group in subsections(&entries) {
        index.push(Object::Integer(i64::from(group[0].id)));
        index.push(Object::Integer(group.len() as i64));
        for entry in group {
            data.push(1);
            data.extend_from_slice(&entry.offset.to_be_bytes()[8 - usize::from(offset_width)..]);
            data.extend_from_slice(&entry.generation.to_be_bytes());
        }
    }

    trailer.set("Type", "XRef");
    trailer.set("Size", Object::Integer(i64::from(xref_id) + 1));
    trailer.set("Index", Object::Array(index));
    trailer.set(
        "W",
        Object::Array(vec![1.into(), i64::from(offset_width).into(), 2.into()]),
    );
    let stream = Stream::new(trailer, data);
    write_indirect_object(out, (xref_id, 0), &Object::Stream(stream))?;
    write!(out, "startxref\n{xref_start}\n%%EOF\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32) -> Entry {
        Entry {
            id,
            generation: 0,
            offset: 0,
        }
    }

    #[test]
    fn subsections_split_on_gaps() {
        let entries: Vec<Entry> = [3, 4, 5, 9, 11, 12].into_iter().map(entry).collect();
        let groups: Vec<Vec<u32>> = subsections(&entries)
            .iter()
            .map(|g| g.iter().map(|e| e.id).collect())
            .collect();
        assert_eq!(groups, vec![vec![3, 4, 5], vec![9], vec![11, 12]]);
        assert!(subsections(&[]).is_empty());
    }

    #[test]
    fn table_entries_are_twenty_bytes() {
        let mut out = CountingWriter::new(Vec::new());
        let entries = [Entry {
            id: 7,
            generation: 2,
            offset: 1234,
        }];
        write_table(&mut out, &entries, Dictionary::new(), 8).unwrap();
        let text = String::from_utf8(out.into_inner()).unwrap();
        let entry = "0000001234 00002 n\r\n";
        assert_eq!(entry.len(), 20);
        assert!(text.contains(&format!("xref\n7 1\n{entry}trailer\n")), "{text:?}");
        assert!(text.contains("/Size 8"), "{text}");
        assert!(text.ends_with("startxref\n0\n%%EOF\n"), "{text}");
    }
}
