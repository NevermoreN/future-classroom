use super::{io_error, Budget, ContainerError, OfficeKind, BLOCK};
use quick_xml::{encoding::DecodingReader, events::Event, Reader, XmlVersion};
use std::collections::BTreeSet;
use std::io::{self, BufRead, BufReader, Read};

const MAX_EVENT: usize = 1024 * 1024;
const MAX_ATTRIBUTE: usize = 64 * 1024;

/// Lexical limits run on UTF-8 bytes BEFORE quick-xml sees a chunk. A large
/// text/tag/attribute therefore cannot grow the parser's event buffer unchecked.
/// DecodingReader uses fixed 8KiB transcoding storage for UTF-8/UTF-16 input.
struct Guard<R> {
    inner: R,
    buf: Box<[u8]>,
    pos: usize,
    len: usize,
    mode: Mode,
    event: usize,
    token: usize,
    quote: Option<u8>,
    quoted: usize,
    prefix: [u8; 9],
    prefix_len: usize,
    last: [u8; 3],
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Text,
    Markup,
    Comment,
    Cdata,
}
impl<R: Read> Guard<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            buf: vec![0; BLOCK].into_boxed_slice(),
            pos: 0,
            len: 0,
            mode: Mode::Text,
            event: 0,
            token: 0,
            quote: None,
            quoted: 0,
            prefix: [0; 9],
            prefix_len: 0,
            last: [0; 3],
        }
    }
    fn byte(&mut self, c: u8) -> Result<(), ContainerError> {
        // Transcoding guarantees UTF-8; XML control bytes are disallowed even in
        // comments, where parsers otherwise tend not to validate character data.
        if c < 32 && !matches!(c, b'\t' | b'\r' | b'\n') {
            return Err(ContainerError::InvalidXml);
        }
        self.last = [self.last[1], self.last[2], c];
        match self.mode {
            Mode::Text => {
                if c == b'<' {
                    self.mode = Mode::Markup;
                    self.event = 1;
                    self.token = 0;
                    self.prefix_len = 1;
                    self.prefix[0] = b'<';
                    self.quote = None;
                } else {
                    self.event += 1;
                    if self.event > MAX_EVENT {
                        return Err(ContainerError::TooLarge);
                    }
                }
            }
            Mode::Markup => {
                self.event += 1;
                if self.event > MAX_EVENT {
                    return Err(ContainerError::TooLarge);
                }
                if self.prefix_len < 9 {
                    self.prefix[self.prefix_len] = c;
                    self.prefix_len += 1;
                }
                let p = &self.prefix[..self.prefix_len];
                if p.starts_with(b"<!") {
                    if p == b"<!--" {
                        self.mode = Mode::Comment;
                    } else if p == b"<![CDATA[" {
                        self.mode = Mode::Cdata;
                    } else if !b"<!--".starts_with(p) && !b"<![CDATA[".starts_with(p) {
                        return Err(ContainerError::ForbiddenContent);
                    }
                }
                if let Some(q) = self.quote {
                    if c == b'<' {
                        return Err(ContainerError::InvalidXml);
                    }
                    if c == q {
                        self.quote = None;
                        self.quoted = 0;
                    } else {
                        self.quoted += 1;
                        if self.quoted > MAX_ATTRIBUTE {
                            return Err(ContainerError::TooLarge);
                        }
                    }
                } else if matches!(c, b'\'' | b'"') {
                    self.quote = Some(c);
                    self.quoted = 0;
                    self.token = 0;
                } else if c == b'>' {
                    self.mode = Mode::Text;
                    self.event = 0;
                    self.token = 0;
                } else if c.is_ascii_whitespace() || matches!(c, b'/' | b'=' | b'?') {
                    self.token = 0;
                } else {
                    self.token += 1;
                    if self.token > 256 {
                        return Err(ContainerError::TooLarge);
                    }
                }
            }
            Mode::Comment | Mode::Cdata => {
                self.event += 1;
                if self.event > MAX_EVENT {
                    return Err(ContainerError::TooLarge);
                }
                let end = if self.mode == Mode::Comment {
                    *b"-->"
                } else {
                    *b"]]>"
                };
                if self.last == end {
                    self.mode = Mode::Text;
                    self.event = 0;
                    self.token = 0;
                }
            }
        }
        Ok(())
    }
}
impl<R: Read> BufRead for Guard<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.pos == self.len {
            self.len = self.inner.read(&mut self.buf)?;
            self.pos = 0;
            for i in 0..self.len {
                self.byte(self.buf[i]).map_err(io_error)?;
            }
        }
        Ok(&self.buf[self.pos..self.len])
    }
    fn consume(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.len);
    }
}
impl<R: Read> Read for Guard<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let b = self.fill_buf()?;
        let n = b.len().min(out.len());
        out[..n].copy_from_slice(&b[..n]);
        self.consume(n);
        Ok(n)
    }
}
fn xml_error(e: quick_xml::Error) -> ContainerError {
    match e {
        quick_xml::Error::Io(e) => e
            .get_ref()
            .and_then(|e| e.downcast_ref::<ContainerError>())
            .copied()
            .unwrap_or(ContainerError::InvalidXml),
        _ => ContainerError::InvalidXml,
    }
}
fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\r' | '\n') || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')
}
fn name_start(c: char) -> bool {
    c == '_'
        || c.is_ascii_alphabetic()
        || matches!(c,'\u{c0}'..='\u{d6}'|'\u{d8}'..='\u{f6}'|'\u{f8}'..='\u{2ff}'|'\u{370}'..='\u{37d}'|'\u{37f}'..='\u{1fff}'|'\u{200c}'..='\u{200d}'|'\u{2070}'..='\u{218f}'|'\u{2c00}'..='\u{2fef}'|'\u{3001}'..='\u{d7ff}'|'\u{f900}'..='\u{fdcf}'|'\u{fdf0}'..='\u{fffd}'|'\u{10000}'..='\u{effff}')
}
fn qualified_name(name: &str) -> bool {
    let mut count = 0;
    for part in name.split(':') {
        count += 1;
        let mut chars = part.chars();
        if count > 2
            || !chars.next().is_some_and(name_start)
            || !chars.all(|c| {
                name_start(c)
                    || c.is_ascii_digit()
                    || matches!(c,'-'|'.'|'\u{b7}'|'\u{300}'..='\u{36f}'|'\u{203f}'..='\u{2040}')
            })
        {
            return false;
        }
    }
    true
}
fn forbidden_type(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    ["macroenabled", "vbaproject", "vba/", "activex", "oleobject"]
        .iter()
        .any(|bad| s.contains(bad))
}
pub(super) fn root_namespace(kind: OfficeKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        OfficeKind::Docx => (
            "document",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
            "http://purl.oclc.org/ooxml/wordprocessingml/main",
        ),
        OfficeKind::Pptx => (
            "presentation",
            "http://schemas.openxmlformats.org/presentationml/2006/main",
            "http://purl.oclc.org/ooxml/presentationml/main",
        ),
        OfficeKind::Xlsx => (
            "workbook",
            "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
            "http://purl.oclc.org/ooxml/spreadsheetml/main",
        ),
    }
}

/// Resolve an OPC internal target without invoking a filesystem or a URL loader.
/// Percent decoding happens before traversal checks; a traversal out of package
/// root, UNC/drive name, NUL or URL scheme is always rejected.
fn target_path(relationship: &str, target: &str) -> Result<String, ContainerError> {
    if target.len() > 2048 {
        return Err(ContainerError::TooLarge);
    }
    let mut decoded = Vec::with_capacity(target.len());
    let mut bytes = target.bytes();
    while let Some(c) = bytes.next() {
        if c == b'%' {
            let h = bytes
                .next()
                .and_then(|v| (v as char).to_digit(16))
                .ok_or(ContainerError::UnsafePath)?;
            let l = bytes
                .next()
                .and_then(|v| (v as char).to_digit(16))
                .ok_or(ContainerError::UnsafePath)?;
            decoded.push((h * 16 + l) as u8);
        } else {
            decoded.push(c)
        }
    }
    let target = std::str::from_utf8(&decoded).map_err(|_| ContainerError::UnsafePath)?;
    if target.contains(['\\', ':', '?', '\0'])
        || target.starts_with("//")
        || target.chars().any(char::is_control)
    {
        return Err(ContainerError::UnsafePath);
    }
    let path = target.split('#').next().unwrap_or("");
    let mut result: Vec<&str> = Vec::new();
    if !path.starts_with('/') && relationship != "_rels/.rels" {
        let (parent, source) = relationship
            .rsplit_once("/_rels/")
            .ok_or(ContainerError::InvalidXml)?;
        result.extend(parent.split('/'));
        if path.is_empty() {
            result.push(
                source
                    .strip_suffix(".rels")
                    .ok_or(ContainerError::InvalidXml)?,
            );
        }
    }
    for part in path.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if result.pop().is_none() {
                    return Err(ContainerError::UnsafePath);
                }
            }
            _ => result.push(part),
        }
    }
    let resolved = result.join("/");
    if resolved.is_empty() || resolved.chars().count() > 512 {
        return Err(ContainerError::UnsafePath);
    }
    Ok(resolved)
}

pub(super) fn validate(
    body: &mut impl Read,
    name: &str,
    kind: OfficeKind,
    budget: &Budget<'_>,
    parts: &BTreeSet<&str>,
) -> Result<(bool, bool), ContainerError> {
    let decoded = DecodingReader::new(BufReader::with_capacity(BLOCK, body));
    let mut reader = Reader::from_reader(Guard::new(decoded));
    reader.config_mut().check_comments = true;
    let mut buffer = Vec::with_capacity(8192);
    let mut depth = 0usize;
    let mut roots = 0u8;
    let mut types_ok = false;
    let mut relationship_ok = false;
    let mut main_root_ok = name != kind.main();
    let types = name == "[Content_Types].xml";
    let rels = name.ends_with(".rels");
    // Bounded namespace scopes avoid an unbounded namespace resolver allocation.
    let mut scopes: Vec<Vec<(String, String)>> = Vec::new();
    let mut ns_bytes = 0usize;
    let mut declaration_allowed = true;
    loop {
        budget.check()?;
        let event = reader.read_event_into(&mut buffer).map_err(xml_error)?;
        if event == Event::Eof {
            break;
        }
        budget.node()?;
        let was_declaration_allowed = declaration_allowed;
        declaration_allowed = false;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                if depth == 0 {
                    roots = roots.checked_add(1).ok_or(ContainerError::InvalidXml)?;
                    if roots > 1 {
                        return Err(ContainerError::InvalidXml);
                    }
                }
                if depth >= 64 {
                    return Err(ContainerError::TooLarge);
                }
                let q = e.name();
                let qname = q.as_ref();
                let (prefix, local) = qname.split_once(':').map_or(("", qname), |(p, l)| (p, l));
                if qname.len() > 256 || !qualified_name(qname) {
                    return Err(ContainerError::InvalidXml);
                }
                let mut declarations = Vec::new();
                let mut attribute_prefixes = Vec::new();
                let mut content_type = None;
                let mut part = None;
                let mut target = None;
                let mut mode = None;
                let mut relation_type = None;
                for (count, attribute) in e.attributes().enumerate() {
                    if count >= 128 {
                        return Err(ContainerError::TooLarge);
                    }
                    let a = attribute.map_err(|_| ContainerError::InvalidXml)?;
                    if !qualified_name(a.key.as_ref()) {
                        return Err(ContainerError::InvalidXml);
                    }
                    if a.key.as_ref().len() > 256 || a.value.len() > MAX_ATTRIBUTE {
                        return Err(ContainerError::TooLarge);
                    }
                    let value = a
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(|_| ContainerError::InvalidXml)?;
                    if !value.chars().all(xml_char) {
                        return Err(ContainerError::InvalidXml);
                    }
                    if a.key.as_ref() == "xmlns" || a.key.as_ref().starts_with("xmlns:") {
                        if value.len() > 2048 {
                            return Err(ContainerError::TooLarge);
                        }
                        let key = a.key.as_ref().strip_prefix("xmlns:").unwrap_or("");
                        if key == "xmlns"
                            || (key == "xml" && value != "http://www.w3.org/XML/1998/namespace")
                            || value == "http://www.w3.org/2000/xmlns/"
                            || (key != "xml" && value == "http://www.w3.org/XML/1998/namespace")
                            || (!key.is_empty() && value.is_empty())
                        {
                            return Err(ContainerError::InvalidXml);
                        }
                        ns_bytes += key.len() + value.len() + 64;
                        if ns_bytes > 64 * 1024 {
                            return Err(ContainerError::TooLarge);
                        }
                        declarations.push((key.to_owned(), value.into_owned()));
                        continue;
                    }
                    if let Some((prefix, _)) = a.key.as_ref().split_once(':') {
                        attribute_prefixes.push(prefix.to_owned());
                    }
                    match a.key.as_ref() {
                        "ContentType" => content_type = Some(value.into_owned()),
                        "PartName" => part = Some(value.into_owned()),
                        "Target" => target = Some(value.into_owned()),
                        "TargetMode" => mode = Some(value.into_owned()),
                        "Type" => relation_type = Some(value.into_owned()),
                        _ => {}
                    }
                }
                scopes.push(declarations);
                for prefix in attribute_prefixes {
                    if prefix != "xml"
                        && !scopes
                            .iter()
                            .rev()
                            .flat_map(|s| s.iter())
                            .any(|(p, _)| p == &prefix)
                    {
                        return Err(ContainerError::InvalidXml);
                    }
                }
                let ns = scopes
                    .iter()
                    .rev()
                    .flat_map(|s| s.iter())
                    .find(|(p, _)| p == prefix)
                    .map(|(_, v)| v.as_str());
                if !prefix.is_empty() && ns.is_none() && prefix != "xml" {
                    return Err(ContainerError::InvalidXml);
                }
                if depth == 0 {
                    if types
                        && (local != "Types"
                            || ns
                                != Some(
                                    "http://schemas.openxmlformats.org/package/2006/content-types",
                                ))
                    {
                        return Err(ContainerError::UnsupportedStructure);
                    }
                    if rels
                        && (local != "Relationships"
                            || !matches!(
                                ns,
                                Some(
                                    "http://schemas.openxmlformats.org/package/2006/relationships"
                                        | "http://purl.oclc.org/ooxml/package/relationships"
                                )
                            ))
                    {
                        return Err(ContainerError::UnsupportedStructure);
                    }
                    if name == kind.main() {
                        let (expected, transitional, strict) = root_namespace(kind);
                        if local != expected || !matches!(ns,Some(s) if s==transitional||s==strict)
                        {
                            return Err(ContainerError::UnsupportedStructure);
                        }
                        main_root_ok = true;
                    }
                }
                if depth > 0
                    && types
                    && (depth != 1
                        || !matches!(local, "Default" | "Override")
                        || ns
                            != Some("http://schemas.openxmlformats.org/package/2006/content-types"))
                {
                    return Err(ContainerError::UnsupportedStructure);
                }
                if depth > 0
                    && rels
                    && (depth != 1
                        || local != "Relationship"
                        || !matches!(
                            ns,
                            Some(
                                "http://schemas.openxmlformats.org/package/2006/relationships"
                                    | "http://purl.oclc.org/ooxml/package/relationships"
                            )
                        ))
                {
                    return Err(ContainerError::UnsupportedStructure);
                }
                if types && local == "Override" {
                    let p = part
                        .as_deref()
                        .and_then(|p| p.strip_prefix('/'))
                        .ok_or(ContainerError::InvalidXml)?;
                    if !parts.contains(p) {
                        return Err(ContainerError::UnsupportedStructure);
                    }
                }
                if let Some(ct) = content_type.as_deref() {
                    if forbidden_type(ct) {
                        return Err(ContainerError::ForbiddenContent);
                    }
                }
                if types
                    && local == "Override"
                    && depth == 1
                    && part.as_deref() == Some(&format!("/{}", kind.main()))
                    && content_type.as_deref() == Some(kind.content_type())
                {
                    types_ok = true;
                }
                if rels && local == "Relationship" {
                    let target = target.as_deref().ok_or(ContainerError::InvalidXml)?;
                    let t = relation_type.as_deref().ok_or(ContainerError::InvalidXml)?;
                    if forbidden_type(t) || t.ends_with("/package") {
                        return Err(ContainerError::ForbiddenContent);
                    }
                    match mode.as_deref() {
                        Some("External") => {
                            let lower = target.to_ascii_lowercase();
                            if !t.ends_with("/hyperlink")
                                || !lower.starts_with("https://")
                                    && !lower.starts_with("http://")
                                    && !lower.starts_with("mailto:")
                            {
                                return Err(ContainerError::ForbiddenContent);
                            }
                        }
                        None | Some("Internal") => {
                            let resolved = target_path(name, target)?;
                            if !parts.contains(resolved.as_str()) {
                                return Err(ContainerError::UnsupportedStructure);
                            }
                            if name == "_rels/.rels"
                                && t.ends_with("/officeDocument")
                                && resolved == kind.main()
                            {
                                relationship_ok = true;
                            }
                        }
                        _ => return Err(ContainerError::InvalidXml),
                    }
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                } else {
                    for (p, v) in scopes.pop().ok_or(ContainerError::InvalidXml)? {
                        ns_bytes -= p.len() + v.len() + 64;
                    }
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or(ContainerError::InvalidXml)?;
                for (p, v) in scopes.pop().ok_or(ContainerError::InvalidXml)? {
                    ns_bytes -= p.len() + v.len() + 64;
                }
            }
            Event::Text(t) => {
                if !t.chars().all(xml_char)
                    || t.contains("]]>")
                    || depth == 0 && !t.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                {
                    return Err(ContainerError::InvalidXml);
                }
            }
            Event::CData(t) => {
                if depth == 0 || !t.chars().all(xml_char) {
                    return Err(ContainerError::InvalidXml);
                }
            }
            Event::GeneralRef(r) => {
                if depth == 0 {
                    return Err(ContainerError::InvalidXml);
                }
                if let Some(c) = r
                    .resolve_char_ref()
                    .map_err(|_| ContainerError::InvalidXml)?
                {
                    if !xml_char(c) {
                        return Err(ContainerError::InvalidXml);
                    }
                } else if quick_xml::escape::resolve_predefined_entity(r.as_ref()).is_none() {
                    return Err(ContainerError::ForbiddenContent);
                }
            }
            Event::DocType(_) => return Err(ContainerError::ForbiddenContent),
            Event::Decl(d) => {
                if !was_declaration_allowed
                    || roots != 0
                    || d.version()
                        .map_err(|_| ContainerError::InvalidXml)?
                        .as_ref()
                        != "1.0"
                {
                    return Err(ContainerError::InvalidXml);
                }
            }
            Event::PI(p) if p.target().eq_ignore_ascii_case("xml-stylesheet") => {
                return Err(ContainerError::ForbiddenContent)
            }
            Event::Comment(t) if !t.chars().all(xml_char) => {
                return Err(ContainerError::InvalidXml)
            }
            _ => {}
        }
        buffer.clear();
    }
    if roots != 1 || depth != 0 || !main_root_ok {
        return Err(ContainerError::InvalidXml);
    }
    Ok((types_ok, relationship_ok))
}
