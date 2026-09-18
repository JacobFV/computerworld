//! The SQLite 3 database file format (<https://www.sqlite.org/fileformat2.html>).
//!
//! Writing lays every table and index out as a freshly built B-tree: 4096-byte pages,
//! UTF-8 text, schema format 4, no freelist and no auto-vacuum, so the file is compact
//! and the same state always produces the same bytes. Reading walks the table B-trees
//! of any rollback-journal database (following overflow chains) and rebuilds indexes
//! from table contents. WITHOUT ROWID tables, triggers and UTF-16 databases are refused
//! by name rather than half-read.
use crate::ast::Stmt;
use crate::schema::{build_table, IndexOrigin, State, View};
use crate::value::{Affinity, Collation, Value};
use crate::SqlError;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;

pub const PAGE_SIZE: usize = 4096;
const HEADER: &[u8; 16] = b"SQLite format 3\0";

fn malformed() -> SqlError {
    SqlError::new("database disk image is malformed").with_code(11)
}

// ----- varints and records -----

pub fn put_varint(v: u64, out: &mut Vec<u8>) {
    if v <= 0x7f {
        out.push(v as u8);
        return;
    }
    if v > 0x00ff_ffff_ffff_ffff {
        let mut buf = [0u8; 9];
        buf[8] = v as u8;
        let mut x = v >> 8;
        for i in (0..8).rev() {
            buf[i] = (x as u8 & 0x7f) | 0x80;
            x >>= 7;
        }
        out.extend_from_slice(&buf);
        return;
    }
    let mut groups = Vec::new();
    let mut x = v;
    while x > 0 {
        groups.push((x & 0x7f) as u8);
        x >>= 7;
    }
    for (i, g) in groups.iter().rev().enumerate() {
        out.push(if i + 1 == groups.len() { *g } else { g | 0x80 });
    }
}
fn varint_len(v: u64) -> usize {
    let mut b = Vec::new();
    put_varint(v, &mut b);
    b.len()
}
pub fn get_varint(b: &[u8]) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    for i in 0..9 {
        let byte = *b.get(i)?;
        if i == 8 {
            return Some(((v << 8) | u64::from(byte), 9));
        }
        v = (v << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}
fn serial(v: &Value) -> (u64, Vec<u8>) {
    match v {
        Value::Null => (0, vec![]),
        Value::Integer(0) => (8, vec![]),
        Value::Integer(1) => (9, vec![]),
        Value::Integer(i) => {
            let i = *i;
            let (t, n) = if (-128..=127).contains(&i) {
                (1, 1)
            } else if (-32768..=32767).contains(&i) {
                (2, 2)
            } else if (-8_388_608..=8_388_607).contains(&i) {
                (3, 3)
            } else if (-2_147_483_648..=2_147_483_647).contains(&i) {
                (4, 4)
            } else if (-140_737_488_355_328..=140_737_488_355_327).contains(&i) {
                (5, 6)
            } else {
                (6, 8)
            };
            (t, i.to_be_bytes()[8 - n..].to_vec())
        }
        Value::Real(r) => (7, r.to_bits().to_be_bytes().to_vec()),
        Value::Text(t) => (13 + 2 * t.len() as u64, t.as_bytes().to_vec()),
        Value::Blob(b) => (12 + 2 * b.len() as u64, b.clone()),
    }
}
pub fn encode_record(values: &[Value]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut body = Vec::new();
    for v in values {
        let (t, bytes) = serial(v);
        put_varint(t, &mut types);
        body.extend(bytes);
    }
    let mut header_len = types.len() + 1;
    while varint_len(header_len as u64) + types.len() != header_len {
        header_len = varint_len(header_len as u64) + types.len();
    }
    let mut out = Vec::with_capacity(header_len + body.len());
    put_varint(header_len as u64, &mut out);
    out.extend(types);
    out.extend(body);
    out
}
pub fn decode_record(b: &[u8]) -> Result<Vec<Value>, SqlError> {
    let (header_len, mut at) = get_varint(b).ok_or_else(malformed)?;
    let header_len = header_len as usize;
    if header_len > b.len() {
        return Err(malformed());
    }
    let mut types = Vec::new();
    while at < header_len {
        let (t, n) = get_varint(&b[at..]).ok_or_else(malformed)?;
        types.push(t);
        at += n;
    }
    let mut pos = header_len;
    let mut out = Vec::with_capacity(types.len());
    for t in types {
        let int = |n: usize, pos: usize| -> Result<i64, SqlError> {
            let s = b.get(pos..pos + n).ok_or_else(malformed)?;
            let mut v: i64 = if s[0] & 0x80 != 0 { -1 } else { 0 };
            for byte in s {
                v = (v << 8) | i64::from(*byte);
            }
            Ok(v)
        };
        let (v, n) = match t {
            0 => (Value::Null, 0),
            1 => (Value::Integer(int(1, pos)?), 1),
            2 => (Value::Integer(int(2, pos)?), 2),
            3 => (Value::Integer(int(3, pos)?), 3),
            4 => (Value::Integer(int(4, pos)?), 4),
            5 => (Value::Integer(int(6, pos)?), 6),
            6 => (Value::Integer(int(8, pos)?), 8),
            7 => {
                let s = b.get(pos..pos + 8).ok_or_else(malformed)?;
                let bits = u64::from_be_bytes(s.try_into().map_err(|_| malformed())?);
                (Value::real(f64::from_bits(bits)), 8)
            }
            8 => (Value::Integer(0), 0),
            9 => (Value::Integer(1), 0),
            10 | 11 => return Err(malformed()),
            t => {
                let n = ((t - 12) / 2) as usize;
                let s = b.get(pos..pos + n).ok_or_else(malformed)?;
                if t % 2 == 0 {
                    (Value::Blob(s.to_vec()), n)
                } else {
                    (Value::Text(String::from_utf8_lossy(s).into_owned()), n)
                }
            }
        };
        out.push(v);
        pos += n;
    }
    Ok(out)
}

// ----- writing -----

struct Pages {
    pages: Vec<Vec<u8>>,
}
impl Pages {
    fn alloc(&mut self) -> u32 {
        self.pages.push(vec![0; PAGE_SIZE]);
        self.pages.len() as u32
    }
    fn set(&mut self, no: u32, data: Vec<u8>) {
        self.pages[no as usize - 1] = data;
    }
}
fn max_local(payload: usize, index: bool) -> usize {
    let u = PAGE_SIZE;
    let x = if index {
        (u - 12) * 64 / 255 - 23
    } else {
        u - 35
    };
    if payload <= x {
        return payload;
    }
    let m = (u - 12) * 32 / 255 - 23;
    let k = m + (payload - m) % (u - 4);
    if k <= x {
        k
    } else {
        m
    }
}
/// The local part of a payload, spilling the rest into an overflow chain.
fn spill(pages: &mut Pages, payload: &[u8], index: bool) -> Vec<u8> {
    let local = max_local(payload.len(), index);
    let mut out = payload[..local].to_vec();
    if local < payload.len() {
        let rest = &payload[local..];
        let chunks: Vec<&[u8]> = rest.chunks(PAGE_SIZE - 4).collect();
        let numbers: Vec<u32> = chunks.iter().map(|_| pages.alloc()).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let mut page = vec![0u8; PAGE_SIZE];
            let next = numbers.get(i + 1).copied().unwrap_or(0);
            page[..4].copy_from_slice(&next.to_be_bytes());
            page[4..4 + chunk.len()].copy_from_slice(chunk);
            pages.set(numbers[i], page);
        }
        out.extend_from_slice(&numbers[0].to_be_bytes());
    }
    out
}
/// Assemble one B-tree page from its cells.
fn page(kind: u8, cells: &[Vec<u8>], right: Option<u32>, header_at: usize) -> Vec<u8> {
    let mut p = vec![0u8; PAGE_SIZE];
    let hdr = if right.is_some() { 12 } else { 8 };
    let mut content = PAGE_SIZE;
    let mut pointers = Vec::new();
    for c in cells {
        content -= c.len();
        p[content..content + c.len()].copy_from_slice(c);
        pointers.push(content as u16);
    }
    p[header_at] = kind;
    p[header_at + 3..header_at + 5].copy_from_slice(&(cells.len() as u16).to_be_bytes());
    let start = if cells.is_empty() { PAGE_SIZE } else { content };
    p[header_at + 5..header_at + 7].copy_from_slice(&((start % 65536) as u16).to_be_bytes());
    if let Some(r) = right {
        p[header_at + 8..header_at + 12].copy_from_slice(&r.to_be_bytes());
    }
    for (i, ptr) in pointers.iter().enumerate() {
        let at = header_at + hdr + i * 2;
        p[at..at + 2].copy_from_slice(&ptr.to_be_bytes());
    }
    p
}
fn capacity(header_at: usize, interior: bool) -> usize {
    PAGE_SIZE - header_at - if interior { 12 } else { 8 }
}
/// Split cells into consecutive groups that each fit on a page (cell + 2-byte pointer).
fn pack(sizes: &[usize], cap: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (i, s) in sizes.iter().enumerate() {
        if used + s + 2 > cap && i > start {
            out.push(start..i);
            start = i;
            used = 0;
        }
        used += s + 2;
    }
    if start < sizes.len() || out.is_empty() {
        out.push(start..sizes.len());
    }
    out
}
/// A table B-tree from `(rowid, record)` in rowid order; `root` forces the root page.
fn table_tree(pages: &mut Pages, rows: Vec<(i64, Vec<u8>)>, root: Option<u32>) -> u32 {
    let root_hdr = if root == Some(1) { 100 } else { 0 };
    let cells: Vec<(i64, Vec<u8>)> = rows
        .into_iter()
        .map(|(id, rec)| {
            let mut c = Vec::new();
            put_varint(rec.len() as u64, &mut c);
            put_varint(id as u64, &mut c);
            c.extend(spill(pages, &rec, false));
            (id, c)
        })
        .collect();
    let sizes: Vec<usize> = cells.iter().map(|(_, c)| c.len()).collect();
    // A single leaf that fits where the root goes is the whole tree.
    if sizes.iter().map(|s| s + 2).sum::<usize>() <= capacity(root_hdr, false) {
        let no = root.unwrap_or_else(|| pages.alloc());
        let data: Vec<Vec<u8>> = cells.into_iter().map(|(_, c)| c).collect();
        pages.set(no, page(0x0D, &data, None, root_hdr));
        return no;
    }
    // Leaves, then interior levels until one fits the root.
    let mut level: Vec<(u32, i64)> = Vec::new();
    for group in pack(&sizes, capacity(0, false)) {
        let no = pages.alloc();
        let data: Vec<Vec<u8>> = cells[group.clone()]
            .iter()
            .map(|(_, c)| c.clone())
            .collect();
        let max = cells[group.end - 1].0;
        pages.set(no, page(0x0D, &data, None, 0));
        level.push((no, max));
    }
    loop {
        let entry = |(child, key): &(u32, i64)| {
            let mut c = child.to_be_bytes().to_vec();
            put_varint(*key as u64, &mut c);
            c
        };
        // All but the last child become cells; the last is the right-most pointer.
        let fits_root = level[..level.len() - 1]
            .iter()
            .map(|e| entry(e).len() + 2)
            .sum::<usize>()
            <= capacity(root_hdr, true);
        if fits_root {
            let no = root.unwrap_or_else(|| pages.alloc());
            let cells: Vec<Vec<u8>> = level[..level.len() - 1].iter().map(entry).collect();
            pages.set(
                no,
                page(0x05, &cells, Some(level[level.len() - 1].0), root_hdr),
            );
            return no;
        }
        // Spread children evenly over pages sized for the widest possible cell (a
        // 4-byte pointer, a 9-byte key and its 2-byte slot), so every page fits and
        // none is left with only a right-most pointer.
        let per_page = capacity(0, true) / 15;
        let pages_needed = level.len().div_ceil(per_page);
        let size = level.len().div_ceil(pages_needed);
        let mut next = Vec::new();
        for members in level.chunks(size) {
            let no = pages.alloc();
            let cells: Vec<Vec<u8>> = members[..members.len() - 1].iter().map(entry).collect();
            pages.set(
                no,
                page(0x05, &cells, Some(members[members.len() - 1].0), 0),
            );
            next.push((no, members[members.len() - 1].1));
        }
        level = next;
    }
}
fn cell_size(prefix: usize, payload: usize, index: bool) -> usize {
    let local = max_local(payload, index);
    prefix + varint_len(payload as u64) + local + if local < payload { 4 } else { 0 }
}
fn index_cell(pages: &mut Pages, child: Option<u32>, rec: &[u8]) -> Vec<u8> {
    let mut c = child.map(|n| n.to_be_bytes().to_vec()).unwrap_or_default();
    put_varint(rec.len() as u64, &mut c);
    c.extend(spill(pages, rec, true));
    c
}
/// Group consecutive cells onto pages. Each group but the last is followed by one
/// item that moves up a level, so no page (and no divider) is ever left empty.
fn group(sizes: &[usize], cap: usize) -> Vec<(usize, usize)> {
    let n = sizes.len();
    let mut out = Vec::new();
    let mut a = 0;
    while a < n {
        let mut b = a;
        let mut used = 0;
        while b < n && (b == a || used + sizes[b] + 2 <= cap) {
            used += sizes[b] + 2;
            b += 1;
        }
        if b + 1 == n && b > a + 1 {
            // One item would be left over: shorten this page so it has a neighbour.
            b -= 1;
        }
        out.push((a, b));
        a = b + 1;
    }
    out
}
/// An index B-tree from records already in index order.
fn index_tree(pages: &mut Pages, records: Vec<Vec<u8>>) -> u32 {
    let sizes: Vec<usize> = records
        .iter()
        .map(|r| cell_size(0, r.len(), true))
        .collect();
    if sizes.iter().map(|s| s + 2).sum::<usize>() <= capacity(0, false) {
        let cells: Vec<Vec<u8>> = records.iter().map(|r| index_cell(pages, None, r)).collect();
        let no = pages.alloc();
        pages.set(no, page(0x0A, &cells, None, 0));
        return no;
    }
    // Leaves, separated by the records between them, which become dividers.
    let mut children = Vec::new();
    let mut dividers: Vec<Vec<u8>> = Vec::new();
    for (a, b) in group(&sizes, capacity(0, false)) {
        let cells: Vec<Vec<u8>> = records[a..b]
            .iter()
            .map(|r| index_cell(pages, None, r))
            .collect();
        let no = pages.alloc();
        pages.set(no, page(0x0A, &cells, None, 0));
        children.push(no);
        if b < records.len() {
            dividers.push(records[b].clone());
        }
    }
    loop {
        let sizes: Vec<usize> = dividers
            .iter()
            .map(|d| cell_size(4, d.len(), true))
            .collect();
        if sizes.iter().map(|s| s + 2).sum::<usize>() <= capacity(0, true) {
            let cells: Vec<Vec<u8>> = (0..dividers.len())
                .map(|i| index_cell(pages, Some(children[i]), &dividers[i]))
                .collect();
            let no = pages.alloc();
            pages.set(
                no,
                page(0x02, &cells, Some(children[children.len() - 1]), 0),
            );
            return no;
        }
        let mut next_children = Vec::new();
        let mut next_dividers = Vec::new();
        for (a, b) in group(&sizes, capacity(0, true)) {
            // Cells a..b pair children with their dividers; child b is the right-most
            // pointer, and divider b (if any) moves up.
            let cells: Vec<Vec<u8>> = (a..b)
                .map(|i| index_cell(pages, Some(children[i]), &dividers[i]))
                .collect();
            let no = pages.alloc();
            pages.set(no, page(0x02, &cells, Some(children[b]), 0));
            next_children.push(no);
            if b < dividers.len() {
                next_dividers.push(dividers[b].clone());
            }
        }
        children = next_children;
        dividers = next_dividers;
    }
}
/// Sort key for an index record, honouring each column's collation and direction.
fn index_order(a: &[Value], b: &[Value], cols: &[(Collation, bool)]) -> Ordering {
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        let (coll, desc) = cols.get(i).copied().unwrap_or((Collation::Binary, false));
        let o = crate::value::compare(x, y, coll);
        let o = if desc { o.reverse() } else { o };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}
fn layout(state: &State) -> (Pages, BTreeMap<String, u32>) {
    let mut pages = Pages {
        pages: vec![vec![0; PAGE_SIZE]],
    };
    let mut roots = BTreeMap::new();
    let mut objects: Vec<(u64, String, bool)> = Vec::new();
    for (k, t) in &state.tables {
        if !t.temp {
            objects.push((t.ordinal, k.clone(), true));
        }
    }
    for (k, i) in &state.indexes {
        if state.tables.get(&i.table).is_some_and(|t| !t.temp) {
            objects.push((i.ordinal, k.clone(), false));
        }
    }
    objects.sort();
    for (_, key, is_table) in &objects {
        if *is_table {
            let t = &state.tables[key];
            let rows = t
                .rows
                .iter()
                .map(|(id, s)| {
                    let mut stored = s.clone();
                    stored.resize(t.columns.len(), Value::Null);
                    if let Some(i) = t.ipk {
                        stored[i] = Value::Null;
                    }
                    (*id, encode_record(&stored))
                })
                .collect();
            roots.insert(key.clone(), table_tree(&mut pages, rows, None));
        } else {
            let index = &state.indexes[key];
            let t = &state.tables[&index.table];
            let mut entries: Vec<Vec<Value>> = t
                .rows
                .iter()
                .map(|(id, s)| {
                    let full = t.full_row(*id, s);
                    let mut e: Vec<Value> = index
                        .columns
                        .iter()
                        .map(|c| full[c.column].clone())
                        .collect();
                    e.push(Value::Integer(*id));
                    e
                })
                .collect();
            let mut cols: Vec<(Collation, bool)> = index
                .columns
                .iter()
                .map(|c| (c.collation, c.desc))
                .collect();
            cols.push((Collation::Binary, false));
            entries.sort_by(|a, b| index_order(a, b, &cols));
            let records = entries.iter().map(|e| encode_record(e)).collect();
            roots.insert(key.clone(), index_tree(&mut pages, records));
        }
    }
    // The schema table, whose root is always page 1.
    let db = crate::Database {
        state: state.clone(),
        ..crate::Database::default()
    };
    let schema: Vec<(i64, Vec<u8>)> = db
        .schema()
        .into_iter()
        .enumerate()
        .map(|(i, e)| {
            let root = roots
                .get(&e.name.to_ascii_lowercase())
                .copied()
                .unwrap_or(0);
            let rec = encode_record(&[
                Value::Text(e.kind),
                Value::Text(e.name),
                Value::Text(e.table),
                Value::Integer(i64::from(root)),
                e.sql.map_or(Value::Null, Value::Text),
            ]);
            (i as i64 + 1, rec)
        })
        .collect();
    table_tree(&mut pages, schema, Some(1));
    (pages, roots)
}
/// Root page of every table and index, as the written file would place them.
pub fn root_pages(state: &State) -> BTreeMap<String, u32> {
    layout(state).1
}
pub fn write(state: &State) -> Vec<u8> {
    let (pages, _) = layout(state);
    let count = pages.pages.len() as u32;
    let mut out: Vec<u8> = pages.pages.concat();
    let h = &mut out[..100];
    h[..16].copy_from_slice(HEADER);
    h[16..18].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
    h[18] = 1;
    h[19] = 1;
    h[20] = 0;
    h[21] = 64;
    h[22] = 32;
    h[23] = 32;
    let counter = state.change_counter.max(1);
    h[24..28].copy_from_slice(&counter.to_be_bytes());
    h[28..32].copy_from_slice(&count.to_be_bytes());
    h[40..44].copy_from_slice(&state.schema_cookie.max(1).to_be_bytes());
    h[44..48].copy_from_slice(&4u32.to_be_bytes());
    h[56..60].copy_from_slice(&1u32.to_be_bytes());
    h[60..64].copy_from_slice(&(state.user_version as i32).to_be_bytes());
    h[92..96].copy_from_slice(&counter.to_be_bytes());
    h[96..100].copy_from_slice(&crate::SQLITE_VERSION_NUMBER.to_be_bytes());
    out
}

// ----- reading -----

struct Reader<'a> {
    bytes: &'a [u8],
    page_size: usize,
    usable: usize,
}
impl Reader<'_> {
    fn page(&self, no: u32) -> Result<&[u8], SqlError> {
        let start = (no as usize).checked_sub(1).ok_or_else(malformed)? * self.page_size;
        self.bytes
            .get(start..start + self.page_size)
            .ok_or_else(malformed)
    }
    fn payload(&self, cell: &[u8], len: usize, index: bool) -> Result<Vec<u8>, SqlError> {
        let u = self.usable;
        let x = if index {
            (u - 12) * 64 / 255 - 23
        } else {
            u - 35
        };
        let local = if len <= x {
            len
        } else {
            let m = (u - 12) * 32 / 255 - 23;
            let k = m + (len - m) % (u - 4);
            if k <= x {
                k
            } else {
                m
            }
        };
        let mut out = cell.get(..local).ok_or_else(malformed)?.to_vec();
        if local < len {
            let mut next = u32::from_be_bytes(
                cell.get(local..local + 4)
                    .ok_or_else(malformed)?
                    .try_into()
                    .map_err(|_| malformed())?,
            );
            let mut guard = 0;
            while out.len() < len {
                guard += 1;
                if next == 0 || guard > 1_000_000 {
                    return Err(malformed());
                }
                let p = self.page(next)?;
                let take = (len - out.len()).min(u - 4);
                out.extend_from_slice(&p[4..4 + take]);
                next = u32::from_be_bytes(p[..4].try_into().map_err(|_| malformed())?);
            }
        }
        Ok(out)
    }
    /// Every (rowid, record) of a table B-tree, in rowid order.
    fn table_rows(
        &self,
        root: u32,
        depth: usize,
        out: &mut Vec<(i64, Vec<u8>)>,
    ) -> Result<(), SqlError> {
        if depth > 64 {
            return Err(malformed());
        }
        let p = self.page(root)?;
        let h = if root == 1 { 100 } else { 0 };
        let kind = p[h];
        let n = u16::from_be_bytes([p[h + 3], p[h + 4]]) as usize;
        let hdr = if kind == 0x05 { 12 } else { 8 };
        let ptr = |i: usize| -> Result<usize, SqlError> {
            let at = h + hdr + i * 2;
            Ok(u16::from_be_bytes([
                *p.get(at).ok_or_else(malformed)?,
                *p.get(at + 1).ok_or_else(malformed)?,
            ]) as usize)
        };
        match kind {
            0x0D => {
                for i in 0..n {
                    let at = ptr(i)?;
                    let cell = p.get(at..).ok_or_else(malformed)?;
                    let (len, a) = get_varint(cell).ok_or_else(malformed)?;
                    let (rowid, b) = get_varint(&cell[a..]).ok_or_else(malformed)?;
                    out.push((
                        rowid as i64,
                        self.payload(&cell[a + b..], len as usize, false)?,
                    ));
                }
            }
            0x05 => {
                for i in 0..n {
                    let at = ptr(i)?;
                    let child = u32::from_be_bytes(
                        p.get(at..at + 4)
                            .ok_or_else(malformed)?
                            .try_into()
                            .map_err(|_| malformed())?,
                    );
                    self.table_rows(child, depth + 1, out)?;
                }
                let right =
                    u32::from_be_bytes(p[h + 8..h + 12].try_into().map_err(|_| malformed())?);
                self.table_rows(right, depth + 1, out)?;
            }
            0x0A | 0x02 => {
                return Err(SqlError::new(
                    "WITHOUT ROWID tables are not supported by this engine",
                ))
            }
            _ => return Err(malformed()),
        }
        Ok(())
    }
}
pub fn read(bytes: &[u8]) -> Result<State, SqlError> {
    if bytes.len() < 100 || &bytes[..16] != HEADER {
        return Err(SqlError::new("file is not a database").with_code(26));
    }
    let raw = u16::from_be_bytes([bytes[16], bytes[17]]) as usize;
    let page_size = if raw == 1 { 65536 } else { raw };
    if !page_size.is_power_of_two() || !(512..=65536).contains(&page_size) {
        return Err(SqlError::new("file is not a database").with_code(26));
    }
    let encoding = u32::from_be_bytes(bytes[56..60].try_into().map_err(|_| malformed())?);
    if encoding > 1 {
        return Err(SqlError::new(
            "UTF-16 databases are not supported by this engine",
        ));
    }
    let reader = Reader {
        bytes,
        page_size,
        usable: page_size - bytes[20] as usize,
    };
    let mut state = State {
        schema_cookie: u32::from_be_bytes(bytes[40..44].try_into().map_err(|_| malformed())?),
        change_counter: u32::from_be_bytes(bytes[24..28].try_into().map_err(|_| malformed())?),
        user_version: i64::from(i32::from_be_bytes(
            bytes[60..64].try_into().map_err(|_| malformed())?,
        )),
        ..State::default()
    };
    let mut schema = Vec::new();
    reader.table_rows(1, 0, &mut schema)?;
    let mut entries = Vec::new();
    for (_, rec) in schema {
        let v = decode_record(&rec)?;
        let text = |i: usize| v.get(i).map(Value::to_text).unwrap_or_default();
        entries.push((
            text(0),
            text(1),
            text(2),
            v.get(3).and_then(Value::to_i64).unwrap_or(0),
            v.get(4).cloned().unwrap_or(Value::Null),
        ));
    }
    // Tables and views first, then the indexes that sit on them.
    for (kind, name, _, root, sql) in &entries {
        match kind.as_str() {
            "table" => {
                let Value::Text(sql) = sql else {
                    return Err(malformed());
                };
                let stmts = crate::parser::parse(sql)?;
                let Some(Stmt::CreateTable(ct)) = stmts.into_iter().next() else {
                    return Err(malformed());
                };
                let (mut table, indexes) = build_table(&mut state, &ct, sql.clone())?;
                let mut rows = Vec::new();
                reader.table_rows(*root as u32, 0, &mut rows)?;
                let defaults: Vec<Value> = table
                    .columns
                    .iter()
                    .map(|c| {
                        c.default
                            .as_deref()
                            .and_then(|d| crate::parser::parse_expr(d).ok())
                            .and_then(|e| match e {
                                crate::ast::Expr::Literal(v) => Some(v),
                                crate::ast::Expr::Unary(crate::ast::UnOp::Neg, inner) => {
                                    match *inner {
                                        crate::ast::Expr::Literal(Value::Integer(i)) => {
                                            Some(Value::Integer(-i))
                                        }
                                        crate::ast::Expr::Literal(Value::Real(r)) => {
                                            Some(Value::Real(-r))
                                        }
                                        _ => None,
                                    }
                                }
                                _ => None,
                            })
                            .map(|v| c.affinity.apply(v))
                            .unwrap_or(Value::Null)
                    })
                    .collect();
                for (id, rec) in rows {
                    let mut vals = decode_record(&rec)?;
                    // Columns added by ALTER TABLE after a row was written read as
                    // their default.
                    while vals.len() < table.columns.len() {
                        vals.push(defaults[vals.len()].clone());
                    }
                    vals.truncate(table.columns.len());
                    for (i, c) in table.columns.iter().enumerate() {
                        if c.affinity == Affinity::Real {
                            if let Value::Integer(n) = vals[i] {
                                vals[i] = Value::Real(n as f64);
                            }
                        }
                    }
                    if let Some(i) = table.ipk {
                        vals[i] = Value::Null;
                    }
                    table.rows.insert(id, vals);
                }
                let key = table.name.to_ascii_lowercase();
                let table = Arc::new(table);
                for mut i in indexes {
                    i.rebuild(&table);
                    state
                        .indexes
                        .insert(i.name.to_ascii_lowercase(), Arc::new(i));
                }
                state.tables.insert(key, table);
            }
            "view" => {
                let Value::Text(sql) = sql else {
                    return Err(malformed());
                };
                let stmts = crate::parser::parse(sql)?;
                let Some(Stmt::CreateView(cv)) = stmts.into_iter().next() else {
                    return Err(malformed());
                };
                let ordinal = state.ordinal();
                state.views.insert(
                    name.to_ascii_lowercase(),
                    View {
                        name: cv.name.clone(),
                        sql: sql.clone(),
                        select: crate::view_select_text(sql),
                        columns: cv.columns.clone(),
                        ordinal,
                    },
                );
            }
            "trigger" => {
                return Err(SqlError::new(format!(
                    "triggers are not supported by this engine (trigger {name})"
                )))
            }
            _ => {}
        }
    }
    for (kind, name, _, _, sql) in &entries {
        if kind != "index" {
            continue;
        }
        match sql {
            // Automatic indexes were recreated with their tables.
            Value::Null => {
                if !state.indexes.contains_key(&name.to_ascii_lowercase()) {
                    return Err(malformed());
                }
            }
            Value::Text(sql) => {
                let stmts = crate::parser::parse(sql)?;
                let Some(Stmt::CreateIndex(mut ci)) = stmts.into_iter().next() else {
                    return Err(malformed());
                };
                ci.sql = sql.clone();
                crate::dml::create_index(&mut state, &ci)?;
            }
            _ => return Err(malformed()),
        }
    }
    debug_assert!(state
        .indexes
        .values()
        .all(|i| i.origin != IndexOrigin::Created || i.sql.is_some()));
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn varints_round_trip() {
        for v in [
            0u64,
            1,
            127,
            128,
            240,
            2287,
            16383,
            16384,
            1 << 35,
            u64::MAX,
            0x00ff_ffff_ffff_ffff,
            0x0100_0000_0000_0000,
        ] {
            let mut b = Vec::new();
            put_varint(v, &mut b);
            assert_eq!(get_varint(&b), Some((v, b.len())), "{v}");
        }
    }
    #[test]
    fn records_round_trip() {
        let vals = vec![
            Value::Null,
            Value::Integer(0),
            Value::Integer(1),
            Value::Integer(-200),
            Value::Integer(1 << 40),
            Value::Integer(i64::MIN),
            Value::Real(2.5),
            Value::Text("héllo".into()),
            Value::Blob(vec![0, 1, 2]),
        ];
        assert_eq!(decode_record(&encode_record(&vals)).unwrap(), vals);
    }
}
