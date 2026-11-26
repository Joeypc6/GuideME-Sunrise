//! Enough SNBT to read AE2's guide scenes (`ImportStructure src="x.snbt"`): a structure's `palette` (Name and
//! Properties) and `blocks` (pos, state). Sunrise's API has no SNBT reader for mods.

#[derive(Clone, Debug)]
pub(crate) enum Tag {
    Compound(Vec<(String, Tag)>),
    List(Vec<Tag>),
    Str(String),
    Num(f64),
}

impl Tag {
    pub(crate) fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(c) => c.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn list(&self) -> &[Tag] {
        match self {
            Tag::List(l) => l,
            _ => &[],
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Tag::Num(n) => Some(*n),
            Tag::Str(s) => s.parse().ok(),
            _ => None,
        }
    }
    fn str(&self) -> Option<String> {
        match self {
            Tag::Str(s) => Some(s.clone()),
            Tag::Num(n) => Some(if n.fract() == 0.0 { format!("{}", *n as i64) } else { n.to_string() }),
            _ => None,
        }
    }
}

pub(crate) struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn word(&mut self) -> String {
        self.ws();
        if let Some(q @ (b'"' | b'\'')) = self.s.get(self.i).copied() {
            self.i += 1;
            let mut out = Vec::new();
            while self.i < self.s.len() && self.s[self.i] != q {
                if self.s[self.i] == b'\\' {
                    self.i += 1;
                }
                if let Some(c) = self.s.get(self.i) {
                    out.push(*c);
                }
                self.i += 1;
            }
            self.i += 1;
            return String::from_utf8_lossy(&out).into_owned();
        }
        let start = self.i;
        while self.i < self.s.len() && !b",:]}[{ \n\r\t".contains(&self.s[self.i]) {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
    }
    fn value(&mut self, depth: u32) -> Option<Tag> {
        if depth > 64 {
            return None;
        }
        match self.peek()? {
            b'{' => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    if self.peek()? == b'}' {
                        self.i += 1;
                        break;
                    }
                    let k = self.word();
                    if self.peek()? != b':' {
                        return None;
                    }
                    self.i += 1;
                    out.push((k, self.value(depth + 1)?));
                    if self.peek()? == b',' {
                        self.i += 1;
                    }
                }
                Some(Tag::Compound(out))
            }
            b'[' => {
                self.i += 1;
                // Typed arrays: [I; 1, 2, 3].
                let save = self.i;
                let w = self.word();
                if !(matches!(w.as_str(), "I" | "B" | "L") && self.peek() == Some(b';')) {
                    self.i = save;
                } else {
                    self.i += 1;
                }
                let mut out = Vec::new();
                loop {
                    if self.peek()? == b']' {
                        self.i += 1;
                        break;
                    }
                    out.push(self.value(depth + 1)?);
                    if self.peek()? == b',' {
                        self.i += 1;
                    }
                }
                Some(Tag::List(out))
            }
            b'"' | b'\'' => Some(Tag::Str(self.word())),
            _ => {
                let w = self.word();
                let n = w.trim_end_matches(|c: char| "bBsSlLfFdD".contains(c));
                Some(match n.parse::<f64>() {
                    Ok(v) if !w.is_empty() => Tag::Num(v),
                    _ => match w.as_str() {
                        "true" => Tag::Num(1.0),
                        "false" => Tag::Num(0.0),
                        _ => Tag::Str(w),
                    },
                })
            }
        }
    }
}

pub(crate) fn parse(text: &str) -> Option<Tag> {
    P { s: text.as_bytes(), i: 0 }.value(0)
}

/// A structure's blocks as (pos, `id[k=v,...]`), air left out.
pub(crate) fn structure(text: &str) -> Option<Vec<([i32; 3], String)>> {
    let root = parse(text)?;
    let palette: Vec<String> = root
        .get("palette")?
        .list()
        .iter()
        .map(|p| {
            let name = p.get("Name").and_then(Tag::str).unwrap_or_default();
            match p.get("Properties") {
                Some(Tag::Compound(props)) if !props.is_empty() => {
                    let kv: Vec<String> = props.iter().map(|(k, v)| format!("{k}={}", v.str().unwrap_or_default())).collect();
                    format!("{name}[{}]", kv.join(","))
                }
                _ => name,
            }
        })
        .collect();
    let mut out = Vec::new();
    for b in root.get("blocks")?.list() {
        let pos: Vec<i32> = b.get("pos")?.list().iter().filter_map(Tag::num).map(|n| n as i32).collect();
        let state = b.get("state").and_then(Tag::num)? as usize;
        let id = palette.get(state)?;
        if pos.len() == 3 && !id.ends_with(":air") && !id.starts_with("minecraft:air") {
            out.push(([pos[0], pos[1], pos[2]], id.clone()));
        }
    }
    Some(out)
}
