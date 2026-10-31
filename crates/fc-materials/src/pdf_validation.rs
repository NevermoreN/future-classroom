//! Strict PDF object validation for the isolated, OS-memory/CPU-limited helper.
//! PdfData accepts Arc<read-only mappings>; no second full-file copy is needed.
//! The parser may allocate inside a call, so cooperative checks supplement (and
//! never replace) the helper process's hard memory/CPU/wall limits.
use crate::{
    office::ValidationControl,
    validation::{Budget, ValidationError},
};
pub use pdf_syntax::PdfData;
use pdf_syntax::{
    object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, Stream},
    reader::{Reader, ReaderContext, ReaderExt},
    Filter, Pdf, PdfLoadLimits,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
const MAX_OBJECTS: usize = 20_000;
const MAX_PAGES: usize = 2_000;
const MAX_STREAM: u64 = 8 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfValidationReport {
    pub bytes: u64,
    pub sha256: [u8; 32],
    pub pages: u32,
    pub objects: u32,
}
fn limits() -> PdfLoadLimits {
    PdfLoadLimits::new()
        .max_object_depth(64)
        .max_stream_bytes(MAX_STREAM)
        .max_image_pixels(8_000_000)
}
fn bad() -> ValidationError {
    ValidationError::InvalidPdf
}
fn white(c: u8) -> bool {
    matches!(c, 0 | 9 | 10 | 12 | 13 | 32)
}
fn delimiter(c: u8) -> bool {
    white(c) || b"()<>[]{}/%".contains(&c)
}
#[derive(Default)]
struct TrailerMeta {
    encrypt: bool,
    prev: Option<usize>,
    xref_stream: Option<usize>,
    size: Option<usize>,
    kind: Option<Vec<u8>>,
}
enum Value {
    Integer(usize),
    Name(Vec<u8>),
    Other,
}
/// This lexer only locates actual trailer/xref dictionaries and skips their
/// scalar metadata. All document-body objects/page trees/ObjStm use pdf-syntax.
struct TrailerLexer<'a, 'b> {
    data: &'a [u8],
    at: usize,
    start: usize,
    nodes: usize,
    budget: &'b Budget<'b>,
}
impl<'a, 'b> TrailerLexer<'a, 'b> {
    fn new(data: &'a [u8], at: usize, budget: &'b Budget<'b>) -> Self {
        Self {
            data,
            at,
            start: at,
            nodes: 0,
            budget,
        }
    }
    fn check(&mut self) -> Result<(), ValidationError> {
        self.nodes += 1;
        if self.nodes > 100_000 || self.at.saturating_sub(self.start) > 512 * 1024 {
            return Err(ValidationError::TooLarge);
        }
        if self.nodes.is_multiple_of(64) {
            self.budget.check()?;
        }
        Ok(())
    }
    fn space(&mut self) {
        loop {
            while self.data.get(self.at).is_some_and(|c| white(*c)) {
                self.at += 1
            }
            if self.data.get(self.at) == Some(&b'%') {
                while self
                    .data
                    .get(self.at)
                    .is_some_and(|c| !matches!(*c, b'\r' | b'\n'))
                {
                    self.at += 1
                }
            } else {
                break;
            }
        }
    }
    fn word(&mut self) -> Result<&'a [u8], ValidationError> {
        self.space();
        self.check()?;
        let begin = self.at;
        while self.data.get(self.at).is_some_and(|c| !delimiter(*c)) {
            self.at += 1;
            if self.at - begin > 64 {
                return Err(ValidationError::TooLarge);
            }
        }
        if begin == self.at {
            return Err(bad());
        }
        Ok(&self.data[begin..self.at])
    }
    fn uint(&mut self) -> Result<usize, ValidationError> {
        let word = self.word()?;
        if word.is_empty() || !word.iter().all(u8::is_ascii_digit) {
            return Err(bad());
        }
        word.iter().try_fold(0usize, |n, c| {
            n.checked_mul(10)
                .and_then(|n| n.checked_add(usize::from(c - b'0')))
                .ok_or(ValidationError::TooLarge)
        })
    }
    fn name(&mut self) -> Result<Vec<u8>, ValidationError> {
        self.space();
        self.check()?;
        if self.data.get(self.at) != Some(&b'/') {
            return Err(bad());
        }
        self.at += 1;
        let begin = self.at;
        while self.data.get(self.at).is_some_and(|c| !delimiter(*c)) {
            self.at += 1;
            if self.at - begin > 256 {
                return Err(ValidationError::TooLarge);
            }
        }
        let raw = &self.data[begin..self.at];
        if raw.is_empty() {
            return Err(bad());
        }
        let mut out = Vec::with_capacity(raw.len());
        let mut at = 0;
        while at < raw.len() {
            if raw[at] == b'#' {
                let pair = raw.get(at + 1..at + 3).ok_or_else(bad)?;
                let value = |c: u8| match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                };
                out.push(value(pair[0]).ok_or_else(bad)? * 16 + value(pair[1]).ok_or_else(bad)?);
                at += 3
            } else {
                out.push(raw[at]);
                at += 1
            }
        }
        Ok(out)
    }
    fn dictionary(&mut self, depth: usize) -> Result<TrailerMeta, ValidationError> {
        self.space();
        if depth > 64 {
            return Err(ValidationError::TooLarge);
        }
        if self.data.get(self.at..self.at + 2) != Some(b"<<") {
            return Err(bad());
        }
        self.at += 2;
        let mut meta = TrailerMeta::default();
        let mut keys = HashSet::new();
        loop {
            self.space();
            self.check()?;
            if self.data.get(self.at..self.at + 2) == Some(b">>") {
                self.at += 2;
                return Ok(meta);
            }
            let key = self.name()?;
            if keys.len() >= 4096 || !keys.insert(key.clone()) {
                return Err(bad());
            }
            let value = self.value(depth + 1)?;
            match key.as_slice() {
                b"Encrypt" => meta.encrypt = true,
                b"Prev" => {
                    let Value::Integer(n) = value else {
                        return Err(bad());
                    };
                    meta.prev = Some(n)
                }
                b"XRefStm" => {
                    let Value::Integer(n) = value else {
                        return Err(bad());
                    };
                    meta.xref_stream = Some(n)
                }
                b"Size" => {
                    let Value::Integer(n) = value else {
                        return Err(bad());
                    };
                    meta.size = Some(n)
                }
                b"Type" => {
                    if let Value::Name(name) = value {
                        meta.kind = Some(name)
                    }
                }
                _ => {}
            }
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, ValidationError> {
        self.space();
        self.check()?;
        if depth > 64 {
            return Err(ValidationError::TooLarge);
        }
        let c = *self.data.get(self.at).ok_or_else(bad)?;
        match c {
            b'<' => {
                if self.data.get(self.at + 1) == Some(&b'<') {
                    self.dictionary(depth)?;
                } else {
                    self.at += 1;
                    let begin = self.at;
                    while let Some(c) = self.data.get(self.at) {
                        if *c == b'>' {
                            self.at += 1;
                            return Ok(Value::Other);
                        }
                        if !white(*c) && !c.is_ascii_hexdigit() {
                            return Err(bad());
                        }
                        self.at += 1;
                        if self.at - begin > 64 * 1024 {
                            return Err(ValidationError::TooLarge);
                        }
                    }
                    return Err(bad());
                }
                Ok(Value::Other)
            }
            b'(' => {
                self.at += 1;
                let begin = self.at;
                let mut nesting = 1usize;
                while let Some(c) = self.data.get(self.at).copied() {
                    self.at += 1;
                    if c == b'\\' {
                        if self.at >= self.data.len() {
                            return Err(bad());
                        }
                        self.at += 1;
                    } else if c == b'(' {
                        nesting += 1;
                        if nesting > 64 {
                            return Err(ValidationError::TooLarge);
                        }
                    } else if c == b')' {
                        nesting -= 1;
                        if nesting == 0 {
                            return Ok(Value::Other);
                        }
                    }
                    if self.at - begin > 64 * 1024 {
                        return Err(ValidationError::TooLarge);
                    }
                }
                Err(bad())
            }
            b'[' => {
                self.at += 1;
                loop {
                    self.space();
                    if self.data.get(self.at) == Some(&b']') {
                        self.at += 1;
                        break;
                    }
                    self.value(depth + 1)?;
                }
                Ok(Value::Other)
            }
            b'/' => Ok(Value::Name(self.name()?)),
            _ => {
                let word = self.word()?;
                if word.iter().all(u8::is_ascii_digit) {
                    let n = word.iter().try_fold(0usize, |n, c| {
                        n.checked_mul(10)
                            .and_then(|n| n.checked_add(usize::from(c - b'0')))
                            .ok_or(ValidationError::TooLarge)
                    })?;
                    let end = self.at;
                    let reference = (|| {
                        self.uint().ok()?;
                        (self.word().ok()? == b"R").then_some(())
                    })();
                    if reference.is_some() {
                        Ok(Value::Other)
                    } else {
                        self.at = end;
                        Ok(Value::Integer(n))
                    }
                } else if matches!(word, b"true" | b"false" | b"null")
                    || word
                        .iter()
                        .all(|c| c.is_ascii_digit() || b"+-.".contains(c))
                {
                    Ok(Value::Other)
                } else {
                    Err(bad())
                }
            }
        }
    }
}
fn object_header(
    data: &[u8],
    offset: usize,
    id: usize,
    generation: usize,
    budget: &Budget<'_>,
) -> Result<usize, ValidationError> {
    if offset == 0 || offset >= data.len() {
        return Err(bad());
    }
    let mut lexer = TrailerLexer::new(data, offset, budget);
    if lexer.uint() != Ok(id) || lexer.uint() != Ok(generation) || lexer.word()? != b"obj" {
        return Err(bad());
    }
    Ok(lexer.at)
}
fn number_bytes(bytes: &[u8]) -> Result<usize, ValidationError> {
    bytes.iter().try_fold(0usize, |n, c| {
        n.checked_mul(256)
            .and_then(|n| n.checked_add(usize::from(*c)))
            .ok_or(ValidationError::TooLarge)
    })
}
fn preflight(data: &[u8], budget: &Budget<'_>) -> Result<(), ValidationError> {
    if data.len() < 20
        || !data.starts_with(b"%PDF-")
        || !matches!(&data[5..8], [b'1', b'.', b'0'..=b'7'] | [b'2', b'.', b'0'])
        || !white(data[8])
        || !data.trim_ascii_end().ends_with(b"%%EOF")
    {
        return Err(bad());
    }
    let start = data.len().saturating_sub(64 * 1024);
    let marker = data[start..]
        .windows(9)
        .rposition(|w| w == b"startxref")
        .map(|n| n + start + 9)
        .ok_or_else(bad)?;
    let mut tail = TrailerLexer::new(data, marker, budget);
    let root = tail.uint()?;
    let mut pending = vec![(root, false, 0usize)];
    let mut seen = HashSet::new();
    let mut active = HashSet::new();
    let mut entries = 0usize;
    let mut normal = Vec::new();
    while let Some((offset, leaving, depth)) = pending.pop() {
        budget.check()?;
        if leaving {
            active.remove(&offset);
            seen.insert(offset);
            continue;
        }
        if seen.contains(&offset) {
            continue;
        }
        if seen.len() + active.len() >= 32
            || depth >= 32
            || !active.insert(offset)
            || offset == 0
            || offset >= data.len()
        {
            return Err(bad());
        }
        let mut lexer = TrailerLexer::new(data, offset, budget);
        lexer.space();
        let meta = if lexer.data.get(lexer.at..lexer.at + 4) == Some(b"xref") {
            if lexer.word()? != b"xref" {
                return Err(bad());
            }
            loop {
                lexer.space();
                if lexer.data.get(lexer.at..lexer.at + 7) == Some(b"trailer") {
                    lexer.word()?;
                    break;
                }
                let first = lexer.uint()?;
                let count = lexer.uint()?;
                entries = entries
                    .checked_add(count)
                    .ok_or(ValidationError::TooLarge)?;
                if entries > MAX_OBJECTS + 1
                    || first.checked_add(count).is_none_or(|n| n > MAX_OBJECTS + 1)
                {
                    return Err(ValidationError::TooLarge);
                }
                for id in first..first + count {
                    let position = lexer.uint()?;
                    let generation = lexer.uint()?;
                    let flag = lexer.word()?;
                    if generation > 65535 || !matches!(flag, b"n" | b"f") {
                        return Err(bad());
                    }
                    if flag == b"n" {
                        normal.push((position, id, generation));
                    }
                }
            }
            lexer.dictionary(0)?
        } else {
            let id = lexer.uint()?;
            let generation = lexer.uint()?;
            if id > MAX_OBJECTS || generation > 65535 || lexer.word()? != b"obj" {
                return Err(bad());
            }
            let dictionary_at = lexer.at;
            let meta = lexer.dictionary(0)?;
            if meta.kind.as_deref() != Some(b"XRef") {
                return Err(bad());
            }
            if meta.encrypt {
                return Err(ValidationError::EncryptedPdf);
            }
            let mut r = Reader::new(&data[dictionary_at..]);
            let ctx = ReaderContext::dummy();
            r.skip_white_spaces_and_comments();
            let stream = r.read_with_context::<Stream<'_>>(&ctx).ok_or_else(bad)?;
            let widths_array = stream
                .dict()
                .get::<Array<'_>>(b"W" as &[u8])
                .ok_or_else(bad)?;
            let mut widths = Vec::new();
            for value in widths_array.raw_iter() {
                let MaybeRef::NotRef(Object::Number(number)) = value else {
                    return Err(bad());
                };
                if number.is_real() {
                    return Err(bad());
                }
                widths.push(u8::try_from(number.as_i64()).map_err(|_| bad())?);
                if widths.len() > 3 {
                    return Err(bad());
                }
            }
            let widths: [u8; 3] = widths.try_into().map_err(|_| bad())?;
            if widths.iter().any(|w| *w > 8) || widths.iter().all(|w| *w == 0) {
                return Err(bad());
            }
            let size = meta.size.ok_or_else(bad)?;
            if size > MAX_OBJECTS + 1 {
                return Err(ValidationError::TooLarge);
            }
            let indexes = if let Some(array) = stream.dict().get::<Array<'_>>(b"Index" as &[u8]) {
                let mut values = Vec::new();
                for value in array.raw_iter() {
                    let MaybeRef::NotRef(Object::Number(number)) = value else {
                        return Err(bad());
                    };
                    if number.is_real() {
                        return Err(bad());
                    }
                    let value = usize::try_from(number.as_i64()).map_err(|_| bad())?;
                    values.push(value);
                    if values.len() > MAX_OBJECTS * 2 {
                        return Err(ValidationError::TooLarge);
                    }
                }
                if values.len() % 2 != 0 {
                    return Err(bad());
                }
                values
            } else {
                vec![0, size]
            };
            preflight_stream_budget(&stream, budget)?;
            let decoded = stream.decoded().map_err(decode_error)?;
            if decoded.len() as u64 > MAX_STREAM {
                return Err(ValidationError::TooLarge);
            }
            let row = widths.iter().map(|n| usize::from(*n)).sum::<usize>();
            let mut at = 0usize;
            for pair in indexes.as_chunks::<2>().0.iter() {
                let first = pair[0];
                let count = pair[1];
                entries = entries
                    .checked_add(count)
                    .ok_or(ValidationError::TooLarge)?;
                if entries > MAX_OBJECTS + 1 || first.checked_add(count).is_none_or(|n| n > size) {
                    return Err(ValidationError::TooLarge);
                }
                for id in first..first + count {
                    budget.check()?;
                    let entry = decoded.get(at..at + row).ok_or_else(bad)?;
                    let w0 = usize::from(widths[0]);
                    let w1 = usize::from(widths[1]);
                    let kind = if w0 == 0 {
                        1
                    } else {
                        number_bytes(&entry[..w0])?
                    };
                    let second = number_bytes(&entry[w0..w0 + w1])?;
                    let third = number_bytes(&entry[w0 + w1..])?;
                    match kind {
                        0 => {}
                        1 => {
                            if third > 65535 {
                                return Err(bad());
                            }
                            normal.push((second, id, third));
                        }
                        2 => {
                            if second == 0 || second > MAX_OBJECTS || third >= MAX_OBJECTS {
                                return Err(bad());
                            }
                        }
                        _ => return Err(bad()),
                    }
                    at += row;
                }
            }
            if at != decoded.len() {
                return Err(bad());
            }
            meta
        };
        if meta.encrypt {
            return Err(ValidationError::EncryptedPdf);
        }
        if meta.size.is_none_or(|n| n == 0 || n > MAX_OBJECTS + 1) {
            return Err(ValidationError::TooLarge);
        }
        pending.push((offset, true, depth));
        if let Some(previous) = meta.prev {
            pending.push((previous, false, depth + 1));
        }
        if let Some(stream) = meta.xref_stream {
            pending.push((stream, false, depth + 1));
        }
    }
    for (offset, id, generation) in normal {
        budget.check()?;
        let at = object_header(data, offset, id, generation, budget)?;
        let mut r = Reader::new(&data[at..]);
        let ctx = ReaderContext::dummy();
        r.skip_white_spaces_and_comments();
        if let Some(dict) = r.read_with_context::<Dict<'_>>(&ctx) {
            if dict.get::<usize>(b"Length" as &[u8]).is_some() {
                let mut stream_reader = Reader::new(&data[at..]);
                stream_reader.skip_white_spaces_and_comments();
                if let Some(stream) = stream_reader
                    .read_with_context::<Stream<'_>>(&ctx)
                    .filter(|s| s.filters() == [Filter::FlateDecode])
                {
                    preflight_stream_budget(&stream, budget)?;
                }
            }
            if dict.get::<Name>(b"Type" as &[u8]).as_deref() == Some(b"ObjStm")
                && (dict
                    .get::<usize>(b"N" as &[u8])
                    .is_none_or(|n| n == 0 || n > MAX_OBJECTS)
                    || dict
                        .get::<usize>(b"First" as &[u8])
                        .is_none_or(|n| n as u64 > MAX_STREAM))
            {
                return Err(ValidationError::TooLarge);
            }
        }
    }
    Ok(())
}

fn preflight_stream_budget(
    stream: &Stream<'_>,
    budget: &Budget<'_>,
) -> Result<(), ValidationError> {
    use std::io::Read;
    let raw = stream.raw_data();
    if raw.len() as u64 > MAX_STREAM {
        return Err(ValidationError::TooLarge);
    }
    if stream.filters().contains(&Filter::Crypt) {
        return Err(ValidationError::EncryptedPdf);
    }
    match stream.filters() {
        [] | [Filter::AsciiHexDecode] | [Filter::Ascii85Decode] => {}
        [Filter::FlateDecode] => {
            let mut decoder = flate2::read::ZlibDecoder::new(raw.as_ref());
            let mut block = [0u8; 32 * 1024];
            let mut total = 0u64;
            loop {
                budget.check()?;
                let n = decoder.read(&mut block).map_err(|_| bad())?;
                if n == 0 {
                    break;
                }
                total = total
                    .checked_add(n as u64)
                    .ok_or(ValidationError::TooLarge)?;
                if total > MAX_STREAM {
                    return Err(ValidationError::TooLarge);
                }
            }
        }
        _ => return Err(bad()),
    }
    Ok(())
}
fn active_name(name: &[u8]) -> bool {
    matches!(
        name,
        b"JavaScript"
            | b"JS"
            | b"Launch"
            | b"EmbeddedFile"
            | b"EmbeddedFiles"
            | b"Filespec"
            | b"GoToR"
            | b"GoToE"
            | b"SubmitForm"
            | b"ImportData"
            | b"Rendition"
            | b"RichMedia"
            | b"Movie"
            | b"Sound"
            | b"URI"
            | b"XFA"
            | b"Crypt"
    )
}
fn active_key(key: &[u8]) -> bool {
    matches!(
        key,
        b"JS"
            | b"JavaScript"
            | b"AA"
            | b"OpenAction"
            | b"EmbeddedFiles"
            | b"EF"
            | b"XFA"
            | b"RichMediaContent"
            | b"RichMediaSettings"
            | b"AcroForm"
    )
}
struct ObjectScan<'a> {
    pdf: &'a Pdf,
    budget: &'a Budget<'a>,
    nodes: usize,
    references: HashSet<ObjectIdentifier>,
    decoded: u64,
}
impl<'a> ObjectScan<'a> {
    fn check(&mut self, depth: usize) -> Result<(), ValidationError> {
        self.budget.check()?;
        self.nodes += 1;
        if depth > 64 || self.nodes > 200_000 {
            return Err(ValidationError::TooLarge);
        }
        Ok(())
    }
    fn reference(&mut self, reference: pdf_syntax::object::ObjRef) -> Result<(), ValidationError> {
        let id = reference.into();
        if self.references.insert(id) && self.pdf.xref().get::<Object<'_>>(id).is_none() {
            return Err(bad());
        }
        Ok(())
    }
    fn maybe(&mut self, value: MaybeRef<Object<'a>>, depth: usize) -> Result<(), ValidationError> {
        match value {
            MaybeRef::Ref(reference) => self.reference(reference),
            MaybeRef::NotRef(object) => self.object(object, depth),
        }
    }
    fn dictionary(&mut self, dict: Dict<'a>, depth: usize) -> Result<(), ValidationError> {
        self.check(depth)?;
        if dict.len() > 4096 {
            return Err(ValidationError::TooLarge);
        }
        for key in dict.keys() {
            self.check(depth)?;
            if active_key(key.as_ref()) {
                return Err(ValidationError::ForbiddenContent);
            }
            let value = dict.get_raw::<Object<'_>>(key.as_ref()).ok_or_else(bad)?;
            self.maybe(value, depth + 1)?;
        }
        Ok(())
    }
    fn filters(&mut self, dict: &Dict<'a>) -> Result<(), ValidationError> {
        let Some(value) = dict.get::<Object<'_>>(b"Filter" as &[u8]) else {
            if dict.contains_key(b"Filter" as &[u8]) {
                return Err(bad());
            }
            return Ok(());
        };
        let names = match value {
            Object::Name(name) => vec![name],
            Object::Array(array) => {
                let mut names = Vec::new();
                for item in array.raw_iter() {
                    let name = match item {
                        MaybeRef::NotRef(Object::Name(name)) => name,
                        MaybeRef::Ref(reference) => self
                            .pdf
                            .xref()
                            .get::<Name>(reference.into())
                            .ok_or_else(bad)?,
                        _ => return Err(bad()),
                    };
                    names.push(name);
                    if names.len() > 8 {
                        return Err(ValidationError::TooLarge);
                    }
                }
                names
            }
            _ => return Err(bad()),
        };
        for name in names {
            if name.as_ref() == b"Crypt" {
                return Err(ValidationError::EncryptedPdf);
            }
            if !matches!(
                name.as_ref(),
                b"FlateDecode"
                    | b"Fl"
                    | b"ASCIIHexDecode"
                    | b"AHx"
                    | b"ASCII85Decode"
                    | b"A85"
                    | b"LZWDecode"
                    | b"LZW"
                    | b"RunLengthDecode"
                    | b"RL"
                    | b"DCTDecode"
                    | b"DCT"
                    | b"JPXDecode"
                    | b"CCITTFaxDecode"
                    | b"CCF"
                    | b"JBIG2Decode"
            ) {
                return Err(bad());
            }
        }
        Ok(())
    }
    fn object(&mut self, object: Object<'a>, depth: usize) -> Result<(), ValidationError> {
        self.check(depth)?;
        match object {
            Object::Name(name) if active_name(name.as_ref()) => {
                return Err(ValidationError::ForbiddenContent)
            }
            Object::Dict(dict) => self.dictionary(dict, depth + 1)?,
            Object::Array(array) => {
                for item in array.raw_iter() {
                    self.maybe(item, depth + 1)?;
                }
            }
            Object::Stream(stream) => {
                self.dictionary(stream.dict().clone(), depth + 1)?;
                if strict_uint(stream.dict(), b"Length")? != stream.raw_data().len() {
                    return Err(bad());
                }
                self.filters(stream.dict())?;
                if stream.filters().contains(&Filter::Crypt) {
                    return Err(ValidationError::EncryptedPdf);
                }
                let image =
                    stream.dict().get::<Name>(b"Subtype" as &[u8]).as_deref() == Some(b"Image");
                if image {
                    let width = strict_uint(stream.dict(), b"Width")?;
                    let height = strict_uint(stream.dict(), b"Height")?;
                    if width == 0
                        || height == 0
                        || width
                            .checked_mul(height)
                            .is_none_or(|pixels| pixels > 8_000_000)
                    {
                        return Err(ValidationError::TooLarge);
                    }
                }
                let image_filter = stream.filters().iter().any(|filter| {
                    matches!(
                        filter,
                        Filter::DctDecode
                            | Filter::JpxDecode
                            | Filter::CcittFaxDecode
                            | Filter::Jbig2Decode
                    )
                });
                if image_filter {
                    if !image || stream.raw_data().len() as u64 > MAX_STREAM {
                        return Err(bad());
                    } // Object/pixel metadata is checked; no automatic image rendering.
                } else {
                    let decoded = stream.decoded().map_err(decode_error)?;
                    if decoded.len() as u64 > MAX_STREAM {
                        return Err(ValidationError::TooLarge);
                    }
                    self.decoded = self
                        .decoded
                        .checked_add(decoded.len() as u64)
                        .ok_or(ValidationError::TooLarge)?;
                    if self.decoded > 32 * 1024 * 1024 {
                        return Err(ValidationError::TooLarge);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
fn strict_uint(dict: &Dict<'_>, key: &[u8]) -> Result<usize, ValidationError> {
    let number = dict
        .get::<pdf_syntax::object::Number>(key)
        .ok_or_else(bad)?;
    if number.is_real() {
        return Err(bad());
    }
    usize::try_from(number.as_i64()).map_err(|_| bad())
}
fn page_tree(
    pdf: &Pdf,
    dict: Dict<'_>,
    parent: Option<ObjectIdentifier>,
    depth: usize,
    seen: &mut HashSet<ObjectIdentifier>,
    budget: &Budget<'_>,
) -> Result<usize, ValidationError> {
    budget.check()?;
    if depth > 64 || seen.len() > MAX_OBJECTS {
        return Err(ValidationError::TooLarge);
    }
    let id = dict.obj_id().ok_or_else(bad)?;
    if !seen.insert(id) {
        return Err(bad());
    }
    if let Some(parent) = parent {
        if dict.get_ref(b"Parent" as &[u8]).map(ObjectIdentifier::from) != Some(parent) {
            return Err(bad());
        }
    }
    match dict.get::<Name>(b"Type" as &[u8]).as_deref() {
        Some(b"Page") => Ok(1),
        Some(b"Pages") => {
            let expected = strict_uint(&dict, b"Count")?;
            if expected > MAX_PAGES {
                return Err(ValidationError::TooLarge);
            }
            let kids = dict.get::<Array<'_>>(b"Kids" as &[u8]).ok_or_else(bad)?;
            let mut actual = 0usize;
            let mut children = 0usize;
            for child in kids.raw_iter() {
                children += 1;
                if children > MAX_PAGES {
                    return Err(ValidationError::TooLarge);
                }
                let MaybeRef::Ref(reference) = child else {
                    return Err(bad());
                };
                let child = pdf
                    .xref()
                    .get::<Dict<'_>>(reference.into())
                    .ok_or_else(bad)?;
                actual = actual
                    .checked_add(page_tree(pdf, child, Some(id), depth + 1, seen, budget)?)
                    .ok_or(ValidationError::TooLarge)?;
                if actual > MAX_PAGES {
                    return Err(ValidationError::TooLarge);
                }
            }
            if actual != expected {
                return Err(bad());
            }
            Ok(actual)
        }
        _ => Err(bad()),
    }
}
/// Only call for untrusted files in the isolated helper process. Cooperative
/// checks cannot interrupt a single third-party parser/decompressor allocation.
pub fn validate_pdf(
    data: impl Into<PdfData>,
    control: &dyn ValidationControl,
) -> Result<PdfValidationReport, ValidationError> {
    let budget = Budget::new(control)?;
    let data = data.into();
    let bytes = data.as_ref();
    if bytes.is_empty() || bytes.len() > 50 * 1024 * 1024 {
        return Err(ValidationError::TooLarge);
    }
    preflight(bytes, &budget)?;
    budget.check()?;
    let _leniency = LeniencyGuard::new();
    let pdf = Pdf::new_with_limits(data.clone(), limits()).map_err(|_| bad())?;
    budget.check()?;
    let recovery = pdf.load_recovery();
    if recovery.xref_rebuilt || recovery.page_tree_rebuilt {
        return Err(bad());
    }
    if pdf.len() > MAX_OBJECTS || pdf.pages().len() > MAX_PAGES {
        return Err(ValidationError::TooLarge);
    }
    if pdf.pages().is_empty() {
        return Err(bad());
    }
    let root = pdf
        .xref()
        .get::<Dict<'_>>(pdf.xref().root_id())
        .ok_or_else(bad)?;
    if root.get::<Name>(b"Type" as &[u8]).as_deref() != Some(b"Catalog") {
        return Err(bad());
    }
    let root_pages = root.get::<Dict<'_>>(b"Pages" as &[u8]).ok_or_else(bad)?;
    let page_count = page_tree(&pdf, root_pages, None, 0, &mut HashSet::new(), &budget)?;
    if page_count != pdf.pages().len() {
        return Err(bad());
    }
    let mut scan = ObjectScan {
        pdf: &pdf,
        budget: &budget,
        nodes: 0,
        references: HashSet::new(),
        decoded: 0,
    };
    let mut count = 0usize;
    for object in pdf.objects() {
        budget.check()?;
        count += 1;
        if count > MAX_OBJECTS {
            return Err(ValidationError::TooLarge);
        }
        scan.object(object, 0)?;
    }
    if count != pdf.len() {
        return Err(bad());
    }
    for page in pdf.pages().iter() {
        budget.check()?;
        let (width, height) = page.base_dimensions();
        if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
            return Err(bad());
        }
    }
    if !pdf_syntax::leniency::drain().is_empty() {
        return Err(bad());
    }
    let mut hash = Sha256::new();
    for block in bytes.chunks(32 * 1024) {
        budget.check()?;
        hash.update(block);
    }
    budget.check()?;
    Ok(PdfValidationReport {
        bytes: bytes.len() as u64,
        sha256: hash.finalize().into(),
        pages: page_count as u32,
        objects: count as u32,
    })
}

fn decode_error(error: pdf_syntax::object::stream::DecodeFailure) -> ValidationError {
    match error {
        pdf_syntax::object::stream::DecodeFailure::StreamTooLarge { .. } => {
            ValidationError::TooLarge
        }
        _ => bad(),
    }
}

struct LeniencyGuard;
impl LeniencyGuard {
    fn new() -> Self {
        pdf_syntax::leniency::activate();
        Self
    }
}
impl Drop for LeniencyGuard {
    fn drop(&mut self) {
        let _ = pdf_syntax::leniency::drain();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::ContainerError;
    use std::{io::Write, time::Duration};
    struct Control;
    impl ValidationControl for Control {
        fn canceled(&self) -> bool {
            false
        }
        fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
            Ok(Duration::ZERO)
        }
    }
    fn zlib(bytes: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }
    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut value = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        value.extend_from_slice(data);
        value.extend_from_slice(b"\nendstream");
        value
    }
    fn objects(extra: &str) -> Vec<Vec<u8>> {
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R {extra} >>"
            )
            .into_bytes(),
            stream("", b"BT (Hello /Encrypt) Tj ET"),
        ]
    }
    fn classic(objects: Vec<Vec<u8>>, trailer: &str) -> (Vec<u8>, usize) {
        let mut data = b"%PDF-1.7\n".to_vec();
        let mut offsets = vec![];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(data.len());
            data.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            data.extend_from_slice(object);
            data.extend_from_slice(b"\nendobj\n");
        }
        let xref = data.len();
        data.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        data.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R {trailer} >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        (data, xref)
    }
    fn object_stream_pdf(active: bool, encryption: &str, n: usize) -> Vec<u8> {
        let mut data = b"%PDF-1.5\n".to_vec();
        let mut offsets = [0usize; 7];
        let page = format!(
            "3 0 << /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R {} >>",
            if active {
                "/AA << /O << /S /JavaScript /JS (evil) >> >>"
            } else {
                ""
            }
        );
        for (id, object) in [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (4, stream("", b"BT (ok) Tj ET")),
            (
                5,
                stream(
                    &format!("/Type /ObjStm /N {n} /First 4 /Filter /FlateDecode"),
                    &zlib(page.as_bytes()),
                ),
            ),
        ] {
            offsets[id] = data.len();
            data.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
            data.extend_from_slice(&object);
            data.extend_from_slice(b"\nendobj\n");
        }
        offsets[6] = data.len();
        let mut xref = Vec::new();
        for (id, offset) in offsets.iter().enumerate() {
            let (kind, second, third) = if id == 0 {
                (0, 0u32, 65535u16)
            } else if id == 3 {
                (2, 5, 0)
            } else {
                (1, *offset as u32, 0)
            };
            xref.push(kind);
            xref.extend_from_slice(&second.to_be_bytes());
            xref.extend_from_slice(&third.to_be_bytes());
        }
        let dict =
            format!("/Type /XRef /Size 7 /Root 1 0 R /W [1 4 2] /Filter /FlateDecode {encryption}");
        data.extend_from_slice(b"6 0 obj\n");
        data.extend_from_slice(&stream(&dict, &zlib(&xref)));
        data.extend_from_slice(format!("\nendobj\nstartxref\n{}\n%%EOF\n", offsets[6]).as_bytes());
        data
    }
    #[test]
    fn genuine_classic_pdf_and_literal_encrypt_text_are_accepted() {
        let data = classic(objects(""), "").0;
        let report = validate_pdf(data.clone(), &Control).unwrap();
        assert_eq!(report.pages, 1);
        assert_eq!(report.objects, 4);
        assert_eq!(report.bytes, data.len() as u64);
        assert_eq!(report.sha256, <Sha256 as Digest>::digest(&data).as_slice());
    }
    #[test]
    fn xref_stream_and_compressed_page_object_are_actually_parsed() {
        let report = validate_pdf(object_stream_pdf(false, "", 1), &Control).unwrap();
        assert_eq!(report.pages, 1);
        assert_eq!(report.objects, 6);
        assert_eq!(
            validate_pdf(object_stream_pdf(true, "", 1), &Control),
            Err(ValidationError::ForbiddenContent)
        );
    }
    #[test]
    fn trailer_encryption_keys_including_hex_and_previous_revision_are_rejected() {
        for key in ["/Encrypt null", "/Encr#79pt 9 0 R", "/E#6ecrypt null"] {
            assert_eq!(
                validate_pdf(classic(objects(""), key).0, &Control),
                Err(ValidationError::EncryptedPdf)
            );
        }
        let (mut old, previous) = classic(objects(""), "/Encrypt null");
        let xref = old.len();
        old.extend_from_slice(format!("xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 5 /Root 1 0 R /Prev {previous} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        assert_eq!(
            validate_pdf(old, &Control),
            Err(ValidationError::EncryptedPdf)
        );
        assert_eq!(
            validate_pdf(object_stream_pdf(false, "/Encr#79pt null", 1), &Control),
            Err(ValidationError::EncryptedPdf)
        );
    }
    #[test]
    fn active_content_is_checked_in_structure_not_compressed_payload_bytes() {
        for active in [
            "/OpenAction 4 0 R",
            "/AA << /O << /S /Launch >> >>",
            "/Names << /EmbeddedFiles [] >>",
        ] {
            assert_eq!(
                validate_pdf(classic(objects(active), "").0, &Control),
                Err(ValidationError::ForbiddenContent)
            );
        }
        let mut docs = objects("");
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
        encoder
            .write_all(b"BT (raw /Encrypt /JavaScript in page text) Tj ET")
            .unwrap();
        docs[3] = stream("/Filter /FlateDecode", &encoder.finish().unwrap());
        validate_pdf(classic(docs, "").0, &Control).unwrap();
    }
    #[test]
    fn broken_xref_page_count_duplicate_pages_and_recovery_are_not_success() {
        let mut docs = objects("");
        docs[1] = b"<< /Type /Pages /Count 2 /Kids [3 0 R] >>".to_vec();
        assert!(validate_pdf(classic(docs, "").0, &Control).is_err());
        let mut docs = objects("");
        docs[1] = b"<< /Type /Pages /Count 2 /Kids [3 0 R 3 0 R] >>".to_vec();
        assert!(validate_pdf(classic(docs, "").0, &Control).is_err());
        let (mut badxref, _) = classic(objects(""), "");
        let at = badxref
            .windows(18)
            .position(|w| w == b"0000000009 00000 n")
            .unwrap();
        badxref[at + 9] = b'7';
        assert!(validate_pdf(badxref, &Control).is_err());
        assert!(
            validate_pdf(b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n%%EOF".to_vec(), &Control).is_err()
        );
    }
    #[test]
    fn explicit_object_page_and_stream_budgets_fail_without_truncation() {
        assert_eq!(
            validate_pdf(object_stream_pdf(false, "", MAX_OBJECTS + 1), &Control),
            Err(ValidationError::TooLarge)
        );
        let mut docs = objects("");
        docs[1] = b"<< /Type /Pages /Count 2001 /Kids [3 0 R] >>".to_vec();
        assert_eq!(
            validate_pdf(classic(docs, "").0, &Control),
            Err(ValidationError::TooLarge)
        );
        let mut docs = objects("");
        docs[3] = stream(
            "/Filter /FlateDecode",
            &zlib(&vec![b'a'; MAX_STREAM as usize + 1]),
        );
        assert_eq!(
            validate_pdf(classic(docs, "").0, &Control),
            Err(ValidationError::TooLarge)
        );
    }
    #[test]
    fn canceled_control_stops_before_parser_loading() {
        struct Canceled;
        impl ValidationControl for Canceled {
            fn canceled(&self) -> bool {
                true
            }
            fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
                Ok(Duration::ZERO)
            }
        }
        assert_eq!(
            validate_pdf(classic(objects(""), "").0, &Canceled),
            Err(ValidationError::Canceled)
        );
    }
    fn hybrid(encryption: &str) -> Vec<u8> {
        let (mut data, previous) = classic(objects(""), "");
        let supplement = data.len();
        let mut row = vec![1];
        row.extend_from_slice(&(supplement as u32).to_be_bytes());
        row.extend_from_slice(&0u16.to_be_bytes());
        data.extend_from_slice(b"6 0 obj\n");
        data.extend_from_slice(&stream(&format!("/Type /XRef /Size 7 /Root 1 0 R /W [1 4 2] /Index [6 1] /Prev {previous} {encryption}"),&row));
        data.extend_from_slice(b"\nendobj\n");
        let root = data.len();
        data.extend_from_slice(format!("xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 7 /Root 1 0 R /Prev {previous} /XRefStm {supplement} >>\nstartxref\n{root}\n%%EOF\n").as_bytes());
        data
    }
    #[test]
    fn hybrid_xref_dag_is_supported_and_supplement_encryption_cannot_hide() {
        let report = validate_pdf(hybrid(""), &Control).unwrap();
        assert_eq!(report.objects, 5);
        assert_eq!(
            validate_pdf(hybrid("/Encr#79pt null"), &Control),
            Err(ValidationError::EncryptedPdf)
        );
        let (bytes, xref) = classic(objects(""), "");
        let cyclic = String::from_utf8(bytes)
            .unwrap()
            .replace("/Root 1 0 R ", &format!("/Root 1 0 R /Prev {xref} "));
        assert!(validate_pdf(cyclic.into_bytes(), &Control).is_err());
    }
    #[test]
    fn unknown_filters_and_stream_length_recovery_are_rejected() {
        let mut docs = objects("");
        docs[3] = stream("/Filter /MadeUpFilter", b"BT (hi) Tj ET");
        assert_eq!(
            validate_pdf(classic(docs, "").0, &Control),
            Err(ValidationError::InvalidPdf)
        );
        let mut docs = objects("");
        docs[3] = b"<< /Length 1 >>\nstream\nBT (longer-than-one) Tj ET\nendstream".to_vec();
        assert!(validate_pdf(classic(docs, "").0, &Control).is_err());
    }
}
