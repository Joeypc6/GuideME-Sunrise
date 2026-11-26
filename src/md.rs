//! The guide's Markdown, with AE2's tags (`appeng.client.guidebook.compiler`): headings, paragraphs (bold, italic,
//! code, links, `<ItemLink>`), lists, quotes, code blocks, tables, rules, `<ItemImage>`, `<BlockImage>`, `<ItemIcon>`,
//! `<ItemGrid>`, `<Row>`/`<Column>`, `<RecipeFor>`, `<Recipe>`, `<GameScene>` with `<ImportStructure>`, `<Block>`
//! and `<IsometricCamera>`, `<SubPages>` and `<CategoryIndex>`. Unknown tags keep their text.

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Link {
    Page(String),
    Item(String),
    Web(String),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<Link>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Scene {
    pub structures: Vec<String>,
    pub blocks: Vec<([i32; 3], String)>,
    /// Block entities drawn with their blocks: (pos, block entity type, SNBT of its saved fields). A `<Block>`'s
    /// `entity="ns:type"` and `nbt='{...}'` (a faucet pouring: `{state:1,render_fluid:{FluidName:"ns:f",Amount:1}}`).
    /// A display, not a simulation: nothing ticks, so a faucet pours forever and the table below never fills.
    pub entities: Vec<([i32; 3], String, String)>,
    pub zoom: f32,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum Block {
    Heading(u8, Vec<Span>),
    Para(Vec<Span>),
    /// A list item: its depth, its number (None: a bullet), its text.
    Item(usize, Option<u32>, Vec<Span>),
    Quote(Vec<Span>),
    Code(String),
    Rule,
    Table(Vec<Vec<Vec<Span>>>),
    /// Item pictures in a row: (item id, scale).
    Items(Vec<(String, f32)>),
    RecipeFor(String),
    Recipe(String),
    Scene(Scene),
    SubPages(Option<String>),
    CategoryIndex(String),
    /// A picture: its alt text and its `src` as written (`pic.png`, `../pic.png`, `/pic.png`, `ns:pic.png`).
    Image(String, String),
}

/// A tag's attribute: `name="v"`, `name={v}` or `name='v'`.
pub(crate) fn attr(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().last();
        let after = &rest[i + name.len()..];
        if before.is_some_and(|c| c.is_whitespace()) && after.trim_start().starts_with('=') {
            let v = after.trim_start()[1..].trim_start();
            let (open, close) = match v.chars().next()? {
                '"' => ('"', '"'),
                '\'' => ('\'', '\''),
                '{' => ('{', '}'),
                _ => return v.split_whitespace().next().map(|s| s.trim_end_matches("/>").to_owned()),
            };
            let v = &v[open.len_utf8()..];
            return v.find(close).map(|end| v[..end].to_owned());
        }
        rest = &rest[i + name.len()..];
    }
    None
}

/// Inline Markdown: `**bold**`, `*italic*`/`_italic_`, `` `code` ``, `[text](link)`, `<ItemLink id="x"/>`,
/// `<br/>`; other tags are dropped, keeping their text.
pub(crate) fn spans(text: &str, ns: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let (mut bold, mut italic) = (false, false);
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<Span>, bold: bool, italic: bool| {
        if !cur.is_empty() {
            out.push(Span { text: std::mem::take(cur), bold, italic, ..Span::default() });
        }
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..].iter().take(400).collect();
        if rest.starts_with("**") || rest.starts_with("__") {
            flush(&mut cur, &mut out, bold, italic);
            bold = !bold;
            i += 2;
        } else if c == '*'
            || (c == '_'
                && ((i == 0 || !chars[i - 1].is_alphanumeric()) && !italic
                    || italic && chars.get(i + 1).is_none_or(|n| !n.is_alphanumeric())))
        {
            flush(&mut cur, &mut out, bold, italic);
            italic = !italic;
            i += 1;
        } else if c == '`' {
            flush(&mut cur, &mut out, bold, italic);
            let end = chars[i + 1..].iter().position(|c| *c == '`').map(|e| i + 1 + e).unwrap_or(chars.len());
            out.push(Span { text: chars[i + 1..end].iter().collect(), code: true, bold, italic, ..Span::default() });
            i = end + 1;
        } else if c == '['
            && let Some(mid) = rest.find("](")
            && let Some(end) = rest[mid..].find(')')
        {
            flush(&mut cur, &mut out, bold, italic);
            let label = &rest[1..mid];
            let target = &rest[mid + 2..mid + end];
            let link = if target.starts_with("http") { Link::Web(target.to_owned()) } else { Link::Page(target.to_owned()) };
            for s in spans(label, ns) {
                out.push(Span { link: Some(link.clone()), ..s });
            }
            i += rest[..mid + end + 1].chars().count();
        } else if c == '<'
            && let Some(end) = rest.find('>')
        {
            flush(&mut cur, &mut out, bold, italic);
            let tag = &rest[..end + 1];
            if tag.starts_with("<ItemLink") {
                let id = crate::book::item_id(ns, &attr(tag, "id").unwrap_or_default());
                out.push(Span { text: format!("\u{1}{id}"), link: Some(Link::Item(id)), bold, italic, ..Span::default() });
            } else if tag.starts_with("<br") {
                out.push(Span { text: "\n".to_owned(), ..Span::default() });
            }
            i += tag.chars().count();
        } else {
            cur.push(c);
            i += 1;
        }
    }
    flush(&mut cur, &mut out, bold, italic);
    out
}

/// The page's blocks.
pub(crate) fn blocks(body: &str, ns: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    let mut para = String::new();
    let flush = |para: &mut String, out: &mut Vec<Block>| {
        if !para.trim().is_empty() {
            out.push(Block::Para(spans(para.trim(), ns)));
        }
        para.clear();
    };
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if t.is_empty() {
            flush(&mut para, &mut out);
            i += 1;
            continue;
        }
        if t.starts_with("```") {
            flush(&mut para, &mut out);
            let mut code = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                code.push(lines[i]);
                i += 1;
            }
            out.push(Block::Code(code.join("\n")));
            i += 1;
            continue;
        }
        if let Some(h) = t.strip_prefix('#') {
            flush(&mut para, &mut out);
            let level = 1 + h.chars().take_while(|c| *c == '#').count() as u8;
            out.push(Block::Heading(level, spans(h.trim_start_matches('#').trim(), ns)));
            i += 1;
            continue;
        }
        if t == "---" || t == "***" {
            flush(&mut para, &mut out);
            out.push(Block::Rule);
            i += 1;
            continue;
        }
        if let Some(q) = t.strip_prefix('>') {
            flush(&mut para, &mut out);
            out.push(Block::Quote(spans(q.trim(), ns)));
            i += 1;
            continue;
        }
        if t.starts_with('|') {
            flush(&mut para, &mut out);
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('|') {
                let row = lines[i].trim().trim_matches('|');
                if !row.chars().all(|c| "-:| ".contains(c)) {
                    rows.push(row.split('|').map(|c| spans(c.trim(), ns)).collect());
                }
                i += 1;
            }
            out.push(Block::Table(rows));
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let bullet = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ "));
        let numbered = t.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if bullet.is_some() || numbered.is_some() {
            flush(&mut para, &mut out);
            let (n, text) = match numbered {
                Some((n, text)) => (n.parse().ok(), text),
                None => (None, bullet.unwrap_or("")),
            };
            // Lines that continue the item, indented under it.
            let mut text = text.to_owned();
            while i + 1 < lines.len() {
                let next = lines[i + 1];
                let nt = next.trim();
                let nindent = next.len() - next.trim_start().len();
                let numbered_next = nt.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
                if nt.is_empty() || nindent <= indent || nt.starts_with("- ") || nt.starts_with("* ") || nt.starts_with('<') || numbered_next {
                    break;
                }
                text.push(' ');
                text.push_str(nt);
                i += 1;
            }
            out.push(Block::Item(indent / 2, n, spans(&text, ns)));
            i += 1;
            continue;
        }
        if let Some(img) = t.strip_prefix("![") {
            flush(&mut para, &mut out);
            let (alt, rest) = img.split_once(']').unwrap_or((img, ""));
            // `(src "title")`: the src is the first word in the brackets.
            let src = rest.strip_prefix('(').and_then(|r| r.split(')').next()).and_then(|r| r.split_whitespace().next()).unwrap_or("");
            out.push(Block::Image(alt.to_owned(), src.to_owned()));
            i += 1;
            continue;
        }
        if t.starts_with('<') && !t.starts_with("<ItemLink") && !t.starts_with("<br") && !t.starts_with("<Color") && !t.starts_with("<a ") {
            flush(&mut para, &mut out);
            let name: String = t[1..].chars().take_while(|c| c.is_alphanumeric()).collect();
            match name.as_str() {
                "GameScene" => {
                    let mut s = Scene { zoom: attr(t, "zoom").and_then(|z| z.parse().ok()).unwrap_or(4.0), yaw: 225.0, pitch: 30.0, ..Scene::default() };
                    let mut j = i;
                    loop {
                        let l = lines[j].trim();
                        if l.starts_with("<ImportStructure") {
                            s.structures.push(attr(l, "src").unwrap_or_default());
                        } else if l.starts_with("<IsometricCamera") {
                            s.yaw = attr(l, "yaw").and_then(|v| v.parse().ok()).unwrap_or(s.yaw);
                            s.pitch = attr(l, "pitch").and_then(|v| v.parse().ok()).unwrap_or(s.pitch);
                        } else if l.starts_with("<Block ") {
                            let p = |k: &str| attr(l, k).and_then(|v| v.parse().ok()).unwrap_or(0);
                            let pos = [p("x"), p("y"), p("z")];
                            s.blocks.push((pos, crate::book::item_id("minecraft", &attr(l, "id").unwrap_or_default())));
                            if let Some(e) = attr(l, "entity") {
                                s.entities.push((pos, crate::book::item_id("minecraft", &e), attr(l, "nbt").unwrap_or_else(|| "{}".to_owned())));
                            }
                        }
                        if l.contains("</GameScene>") || (j == i && l.ends_with("/>")) || j + 1 >= lines.len() {
                            break;
                        }
                        j += 1;
                    }
                    out.push(Block::Scene(s));
                    i = j + 1;
                    continue;
                }
                "ItemImage" | "BlockImage" | "ItemIcon" | "Row" | "Column" | "ItemGrid" => {
                    // A run of pictures (in a Row or alone) becomes one row.
                    let mut items = Vec::new();
                    let mut j = i;
                    let open_row = matches!(name.as_str(), "Row" | "Column" | "ItemGrid");
                    loop {
                        let l = lines[j].trim();
                        for part in l.split('<').skip(1) {
                            let tag = format!("<{part}");
                            if part.starts_with("ItemImage") || part.starts_with("BlockImage") || part.starts_with("ItemIcon") {
                                let scale = attr(&tag, "scale").and_then(|v| v.parse().ok()).unwrap_or(if part.starts_with("ItemIcon") { 1.0 } else { 2.0 });
                                items.push((crate::book::item_id(ns, &attr(&tag, "id").unwrap_or_default()), scale));
                            }
                        }
                        if !open_row || l.starts_with("</Row") || l.starts_with("</Column") || l.starts_with("</ItemGrid") || j + 1 >= lines.len() {
                            break;
                        }
                        j += 1;
                    }
                    if !items.is_empty() {
                        out.push(Block::Items(items));
                    }
                    i = j + 1;
                    continue;
                }
                "img" => out.push(Block::Image(attr(t, "alt").unwrap_or_default(), attr(t, "src").unwrap_or_default())),
                "RecipeFor" => out.push(Block::RecipeFor(crate::book::item_id(ns, &attr(t, "id").unwrap_or_default()))),
                "Recipe" => out.push(Block::Recipe(crate::book::item_id(ns, &attr(t, "id").unwrap_or_default()))),
                "SubPages" => out.push(Block::SubPages(attr(t, "id"))),
                "CategoryIndex" => out.push(Block::CategoryIndex(attr(t, "category").unwrap_or_default())),
                _ => {
                    // An unknown tag: its inner text, if any, as a paragraph.
                    let inner = t.split_once('>').map(|(_, r)| r).unwrap_or("");
                    let inner = inner.split("</").next().unwrap_or("");
                    if !inner.trim().is_empty() {
                        out.push(Block::Para(spans(inner, ns)));
                    }
                }
            }
            i += 1;
            continue;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(t);
        // Two trailing spaces or a backslash break the line.
        if line.ends_with("  ") || line.ends_with('\\') {
            para.push_str("<br/>");
        }
        i += 1;
    }
    flush(&mut para, &mut out);
    out
}
