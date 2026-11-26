//! Every mod's guide pages, read from `assets/<namespace>/ae2guide/` by a client reload listener (Sunrise's
//! `add_client_reload_listeners`), so a mod adds a guide with files only: AE2's guidebook layout (Markdown pages with
//! their front matter, `_<lang>/` translations, structure scenes), plus the optional `index.json` our upstream tools
//! write and a `guide.json` naming the guide.

use mod_api::serde_json::{self, Value};
use mod_api::*;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub(crate) struct Nav {
    pub title: String,
    pub parent: Option<String>,
    pub position: i32,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Page {
    pub nav: Nav,
    /// The body, front matter taken off.
    pub body: String,
    pub items: Vec<String>,
    pub categories: Vec<String>,
    /// An item of each other mod the page is about (`requires:` in the front matter or the index): the page is left
    /// out of the navigation unless they are all in the game.
    pub requires: Vec<String>,
    /// A commands page's operator level (`level:` in the front matter, 0 to 4): the commands menu shows it only to a
    /// player the server gives that level or more.
    pub level: u8,
}

/// One namespace's guide.
#[derive(Clone, Debug, Default)]
pub(crate) struct Guide {
    pub ns: String,
    /// Its name: a translation key or the text (`guide.json` "title"); `<ns>.guide.title` by default.
    pub title: String,
    pub start: String,
    /// Pages by path (`index.md`, `items/drive.md`), in English, and the translations by language then path.
    pub pages: BTreeMap<String, Page>,
    pub translations: HashMap<String, BTreeMap<String, Page>>,
    /// Scenes (structures) by path, as block lists: (x, y, z, block state id with properties).
    pub scenes: HashMap<String, Vec<([i32; 3], String)>>,
    /// The block entities of those scenes (a JSON scene's 6th element, `{"entity": type, "nbt": "{SNBT}"}`):
    /// (pos, block entity type, SNBT), drawn as `<Block entity=... nbt=...>` is.
    pub scene_entities: HashMap<String, Vec<([i32; 3], String, String)>>,
    /// More recipe types `<RecipeFor>` looks in, besides vanilla's (`guide.json` "recipe_types").
    pub recipe_types: Vec<String>,
    /// How a recipe box draws a mod's own recipe type (`guide.json` "recipe_layouts": {type: layout}): a layout is
    /// `{"machine": "ns:item", "label": "key or text", "inputs": ["cast", "fluid"], "outputs": ["result"]}`, each
    /// entry a key of the recipe's JSON (`inputs.0` for a list's first) holding an ingredient, an item or a fluid.
    /// All optional; a type with none is drawn from its JSON's usual keys. Older guides without it are unchanged.
    pub layouts: HashMap<String, Value>,
    /// A mod's commands menu (`assets/<ns>/commands/`, opened by `/<ns>`), not its guide: W and `/guideme` skip it.
    pub commands: bool,
}

impl Guide {
    /// The page in `lang`, else English.
    pub(crate) fn page(&self, path: &str, lang: &str) -> Option<&Page> {
        self.translations.get(lang).and_then(|t| t.get(path)).or_else(|| self.pages.get(path))
    }
    /// Pages in navigation order under `parent` (None: the top).
    pub(crate) fn children(&self, parent: Option<&str>) -> Vec<&String> {
        let mut out: Vec<&String> = self.pages.iter().filter(|(_, p)| p.nav.parent.as_deref() == parent).map(|(k, _)| k).collect();
        out.sort_by_key(|k| (self.pages[*k].nav.position, (*k).clone()));
        out
    }
}

pub(crate) static GUIDES: Mutex<Vec<Guide>> = Mutex::new(Vec::new());

pub(crate) fn with_guides<T>(f: impl FnOnce(&[Guide]) -> T) -> T {
    f(&GUIDES.lock().unwrap_or_else(|e| e.into_inner()))
}

/// The commands books as read (every page, every level); the menu opens a copy cut to the player's level.
pub(crate) static COMMAND_BOOKS: Mutex<Vec<Guide>> = Mutex::new(Vec::new());

pub(crate) struct Listener {
    commands: bool,
}

impl ClientReloadListener for Listener {
    fn prepare(&self, context: &ClientReloadContext) -> Result<Box<dyn FnOnce() + Send + 'static>, ReloadResourceError> {
        let mut by_ns: BTreeMap<String, Vec<(String, Vec<u8>)>> = BTreeMap::new();
        for id in context.list() {
            if let Some(bytes) = context.read(id) {
                by_ns.entry(id.namespace().to_owned()).or_default().push((id.path().to_owned(), bytes.to_vec()));
            }
        }
        let commands = self.commands;
        let mut guides: Vec<Guide> = by_ns.into_iter().filter_map(|(ns, files)| load(&ns, files)).collect();
        for g in &mut guides {
            g.commands = commands;
        }
        Ok(Box::new(move || {
            if commands {
                *COMMAND_BOOKS.lock().unwrap_or_else(|e| e.into_inner()) = guides;
            } else {
                *GUIDES.lock().unwrap_or_else(|e| e.into_inner()) = guides;
            }
        }))
    }
}

/// The guides' listener (`ae2guide/`), or with `commands` the commands menus' (`commands/`).
pub(crate) fn listener(commands: bool) -> Arc<Listener> {
    Arc::new(Listener { commands })
}

/// `namespace`'s commands book cut to `level`: the pages above it are left out (a page whose parent went moves to the
/// top), in every language. None when the mod ships no commands menu.
pub(crate) fn commands_for(namespace: &str, level: u8) -> Option<Guide> {
    let books = COMMAND_BOOKS.lock().unwrap_or_else(|e| e.into_inner());
    let mut g = books.iter().find(|g| g.ns == namespace)?.clone();
    g.pages.retain(|_, p| p.level <= level);
    let kept: std::collections::HashSet<String> = g.pages.keys().cloned().collect();
    for t in g.translations.values_mut() {
        t.retain(|k, _| kept.contains(k));
    }
    for p in g.pages.values_mut().chain(g.translations.values_mut().flat_map(|t| t.values_mut())) {
        if p.nav.parent.as_ref().is_some_and(|parent| !kept.contains(parent)) {
            p.nav.parent = None;
        }
    }
    if !g.pages.contains_key(&g.start) {
        g.start = g.children(None).first().map(|s| (*s).clone()).unwrap_or_default();
    }
    Some(g)
}

fn load(ns: &str, files: Vec<(String, Vec<u8>)>) -> Option<Guide> {
    let mut g = Guide { ns: ns.to_owned(), title: format!("{ns}.guide.title"), start: "index.md".to_owned(), ..Guide::default() };
    let mut index: Option<Value> = None;
    for (path, bytes) in &files {
        let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
        if path == "guide.json" {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                if let Some(t) = v["title"].as_str() {
                    g.title = t.to_owned();
                }
                if let Some(s) = v["start"].as_str() {
                    g.start = s.to_owned();
                }
                if let Some(m) = v["recipe_layouts"].as_object() {
                    g.layouts = m.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    for k in m.keys() {
                        if !v["recipe_types"].as_array().is_some_and(|a| a.iter().any(|x| x == k)) {
                            g.recipe_types.push(k.clone());
                        }
                    }
                }
                g.recipe_types.extend(v["recipe_types"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned));
            }
        } else if path == "index.json" {
            index = serde_json::from_str(&text).ok();
        } else if path.ends_with(".md") {
            let page = parse_page(&text);
            match path.strip_prefix('_').and_then(|p| p.split_once('/')) {
                Some((lang, rest)) => {
                    g.translations.entry(lang.to_lowercase()).or_default().insert(rest.to_owned(), page);
                }
                None => {
                    g.pages.insert(path.clone(), page);
                }
            }
        } else if path.ends_with(".snbt") {
            if let Some(blocks) = crate::snbt::structure(&text) {
                g.scenes.insert(path.clone(), blocks);
            }
        } else if path.ends_with(".json")
            && let Ok(v) = serde_json::from_str::<Value>(&text)
            && v["blocks"].is_array()
        {
            // Our upstream tools' scene form: [x, y, z, "id[props]"] or [x, y, z, "id", {props}], and a block entity
            // after the properties: [x, y, z, "id", {props}, {"entity": "ns:type", "nbt": "{...}"}].
            let entities: Vec<([i32; 3], String, String)> = v["blocks"].as_array().into_iter().flatten().filter_map(|b| {
                let a = b.as_array()?;
                let e = a.get(5)?;
                let pos = [a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32, a.get(2)?.as_i64()? as i32];
                Some((pos, e["entity"].as_str()?.to_owned(), e["nbt"].as_str().unwrap_or("{}").to_owned()))
            }).collect();
            if !entities.is_empty() {
                g.scene_entities.insert(path.clone(), entities);
            }
            let blocks = v["blocks"].as_array().into_iter().flatten().filter_map(|b| {
                let a = b.as_array()?;
                let pos = [a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32, a.get(2)?.as_i64()? as i32];
                let mut id = a.get(3)?.as_str()?.to_owned();
                if let Some(props) = a.get(4).and_then(Value::as_object).filter(|p| !p.is_empty()) {
                    let p: Vec<String> = props.iter().map(|(k, v)| format!("{k}={}", v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))).collect();
                    id = format!("{id}[{}]", p.join(","));
                }
                Some((pos, id))
            }).collect();
            g.scenes.insert(path.clone(), blocks);
        }
    }
    if g.pages.is_empty() {
        return None;
    }
    // The index our tools write fills what a page's front matter left out.
    if let Some(ix) = index {
        let get = |k: &str| ix[k].as_object().cloned().unwrap_or_default();
        for (page, title) in get("titles") {
            if let (Some(p), Some(t)) = (g.pages.get_mut(&page), title.as_str())
                && p.nav.title.is_empty()
            {
                p.nav.title = t.to_owned();
            }
        }
        for (page, parent) in get("parents") {
            if let (Some(p), Some(t)) = (g.pages.get_mut(&page), parent.as_str())
                && p.nav.parent.is_none()
            {
                p.nav.parent = Some(t.to_owned());
            }
        }
        for (page, pos) in get("positions") {
            if let Some(p) = g.pages.get_mut(&page) {
                p.nav.position = pos.as_str().and_then(|s| s.parse().ok()).or(pos.as_i64().map(|n| n as i32)).unwrap_or(p.nav.position);
            }
        }
        for (page, icon) in get("icons") {
            if let (Some(p), Some(t)) = (g.pages.get_mut(&page), icon.as_str())
                && p.nav.icon.is_none()
            {
                p.nav.icon = Some(t.to_owned());
            }
        }
        for (page, items) in get("requires") {
            if let Some(p) = g.pages.get_mut(&page) {
                for i in items.as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
                    if !p.requires.iter().any(|r| r == i) {
                        p.requires.push(i.to_owned());
                    }
                }
            }
        }
        for (item, page) in get("items") {
            if let Some(p) = page.as_str().and_then(|page| g.pages.get_mut(page))
                && !p.items.contains(&item)
            {
                p.items.push(item);
            }
        }
        // `titles_<lang>`: translated titles without a translated page.
        for (key, map) in ix.as_object().into_iter().flatten() {
            if let Some(lang) = key.strip_prefix("titles_") {
                for (page, title) in map.as_object().into_iter().flatten() {
                    let Some(en) = g.pages.get(page).cloned() else { continue };
                    let t = g.translations.entry(lang.to_lowercase()).or_default();
                    let entry = t.entry(page.clone()).or_insert(en);
                    if let Some(title) = title.as_str() {
                        entry.nav.title = title.to_owned();
                    }
                }
            }
        }
    }
    // A page with no title takes its first heading's.
    for p in g.pages.values_mut().chain(g.translations.values_mut().flat_map(|t| t.values_mut())) {
        if p.nav.title.is_empty() {
            p.nav.title = p.body.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or("?").trim().to_owned();
        }
    }
    // Translations keep the English page's place in the navigation.
    let english: HashMap<String, Nav> = g.pages.iter().map(|(k, p)| (k.clone(), p.nav.clone())).collect();
    for t in g.translations.values_mut() {
        for (k, p) in t.iter_mut() {
            if let Some(en) = english.get(k) {
                p.nav.parent = en.parent.clone();
                p.nav.position = en.position;
                p.nav.icon = p.nav.icon.clone().or_else(|| en.icon.clone());
            }
            if p.items.is_empty() {
                p.items = g.pages.get(k).map(|e| e.items.clone()).unwrap_or_default();
            }
        }
    }
    if !g.pages.contains_key(&g.start) {
        g.start = g.children(None).first().map(|s| (*s).clone()).unwrap_or_default();
    }
    Some(g)
}

/// A page: its YAML front matter (navigation, item_ids, categories) and body.
pub(crate) fn parse_page(text: &str) -> Page {
    let mut page = Page::default();
    let mut body = text;
    if let Some(rest) = text.strip_prefix("---\n")
        && let Some(end) = rest.find("\n---")
    {
        let front = &rest[..end];
        body = rest[end + 4..].trim_start_matches('\n');
        let mut list: Option<&str> = None;
        for line in front.lines() {
            let t = line.trim();
            if let Some(item) = t.strip_prefix("- ") {
                let v = unquote(item);
                match list {
                    Some("item_ids") => page.items.push(v),
                    Some("categories") => page.categories.push(v),
                    Some("requires") => page.requires.push(v),
                    _ => {}
                }
                continue;
            }
            let Some((k, v)) = t.split_once(':') else { continue };
            let (k, v) = (k.trim(), unquote(v.trim()));
            list = None;
            match k {
                "title" => page.nav.title = v,
                "parent" => page.nav.parent = Some(v),
                "position" => page.nav.position = v.parse().unwrap_or(0),
                "icon" => page.nav.icon = Some(v),
                "level" => page.level = v.parse::<u8>().unwrap_or(0).min(4),
                "item_ids" | "categories" | "requires" => {
                    if v.starts_with('[') {
                        for x in v.trim_matches(|c| c == '[' || c == ']').split(',') {
                            let x = unquote(x.trim());
                            if !x.is_empty() {
                                match k {
                                    "item_ids" => page.items.push(x),
                                    "requires" => page.requires.push(x),
                                    _ => page.categories.push(x),
                                }
                            }
                        }
                    } else {
                        list = Some(match k {
                            "item_ids" => "item_ids",
                            "requires" => "requires",
                            _ => "categories",
                        });
                    }
                }
                _ => {}
            }
        }
    }
    page.body = body.to_owned();
    page
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_owned()
}

/// A page path relative to another (`../items/x.md`), as the guide's links are written.
pub(crate) fn resolve(from: &str, link: &str) -> String {
    let link = link.split('#').next().unwrap_or(link);
    if let Some(abs) = link.strip_prefix('/') {
        return abs.to_owned();
    }
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for p in link.split('/') {
        match p {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// An item id as pages write them: a bare one is the guide's own namespace.
pub(crate) fn item_id(ns: &str, id: &str) -> String {
    if id.contains(':') { id.to_owned() } else { format!("{ns}:{id}") }
}

/// The guide and page for an item, if a guide has one.
pub(crate) fn page_of_item(id: &str) -> Option<(usize, String)> {
    with_guides(|gs| {
        for (gi, g) in gs.iter().enumerate().filter(|(_, g)| !g.commands) {
            for (path, p) in &g.pages {
                if p.items.iter().any(|i| item_id(&g.ns, i) == id) {
                    return Some((gi, path.clone()));
                }
            }
        }
        None
    })
}
