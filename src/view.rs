//! The guide's window: the navigation on the left (the guide's page tree), the page on the right, a title bar with
//! Back, Contents and the close cross. Drawn in `screen_render_post`, over the open container screen (W over an item)
//! or over the mod's own screen (`/guideme`, `mod_screen_open`). Scenes are Sunrise's `DrawContext::draw_scene`:
//! drag to turn them, scroll over one to zoom.

use crate::book::{self, Guide, with_guides};
use crate::md::{self, Block, Link, Span};
use mod_api::serde_json::Value;
use mod_api::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const BG: u32 = 0xF01A_1B22;
const NAV_BG: u32 = 0xF022_2430;
const EDGE: u32 = 0xFF4A_4E60;
const TEXT: u32 = 0xFFD8_D8D8;
const DIM: u32 = 0xFF90_90A0;
const LINK: u32 = 0xFF6F_A8FF;
const HEAD: u32 = 0xFFFF_E080;
const NAV_W: f32 = 132.0;
const LINE: f32 = 10.0;

#[derive(Clone, Debug)]
pub(crate) enum Target {
    Page(usize, String),
    Item(String),
    Back,
    Contents,
    Close,
    Scene(usize),
    Nav,
    /// A tooltip of its own (a fluid slot's name and amount).
    Tip(String),
}

#[derive(Default)]
pub(crate) struct View {
    pub open: bool,
    /// Opened with `mod_screen_open` (our own screen), not over another.
    pub own_screen: bool,
    pub guide: usize,
    pub page: String,
    pub history: Vec<(usize, String)>,
    pub scroll: f32,
    pub nav_scroll: f32,
    /// Each scene's turn and zoom on this page, by its order: (yaw, pitch, zoom).
    pub scenes: HashMap<usize, (f32, f32, f32)>,
    pub dragging: Option<usize>,
    pub targets: Vec<([f32; 4], Target)>,
    pub rect: [f32; 4],
    pub content: [f32; 4],
    pub nav: [f32; 4],
    /// The page height last frame, to keep the scroll in range.
    pub page_height: f32,
    pub nav_height: f32,
    /// An open asked for by a command, done on the next frame (it needs the mutable host).
    pub pending: Option<(usize, String)>,
    pub lang: String,
    /// What the pointer is over this frame, its tooltip drawn after the locks are released.
    pub hover: Option<Target>,
}

static VIEW: Mutex<Option<View>> = Mutex::new(None);

pub(crate) fn with<T>(f: impl FnOnce(&mut View) -> T) -> T {
    let mut g = VIEW.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(View::default))
}

pub(crate) fn inside(r: [f32; 4], at: [f32; 2]) -> bool {
    at[0] >= r[0] && at[0] < r[0] + r[2] && at[1] >= r[1] && at[1] < r[1] + r[3]
}

impl View {
    pub(crate) fn go(&mut self, guide: usize, page: String) {
        if self.open {
            self.history.push((self.guide, std::mem::take(&mut self.page)));
        }
        self.open = true;
        self.guide = guide;
        self.page = page;
        self.scroll = 0.0;
        self.scenes.clear();
    }
    pub(crate) fn back(&mut self) {
        if let Some((g, p)) = self.history.pop() {
            self.guide = g;
            self.page = p;
            self.scroll = 0.0;
            self.scenes.clear();
        }
    }
    pub(crate) fn close(&mut self) {
        self.open = false;
        self.own_screen = false;
        self.history.clear();
        self.dragging = None;
    }
    pub(crate) fn target_at(&self, at: [f32; 2]) -> Option<Target> {
        self.targets.iter().rev().find(|(r, _)| inside(*r, at)).map(|(_, t)| t.clone())
    }
}

/// `<IfMods items="a:x,b:y">...</IfMods>`: the part shows only when every item named is in the game (one item of
/// each other mod a page talks about, as our guide tools write it); the tags themselves never show.
pub(crate) fn if_mods(host: &dyn Host, body: &str) -> String {
    if !body.contains("<IfMods") {
        return body.to_owned();
    }
    let mut out = String::new();
    let mut rest = body;
    while let Some(start) = rest.find("<IfMods") {
        out.push_str(&rest[..start]);
        let head_end = rest[start..].find('>').map_or(rest.len(), |e| start + e + 1);
        let head = &rest[start..head_end];
        let close = rest[head_end..].find("</IfMods>").map_or(rest.len(), |e| head_end + e);
        let items = head.split("items=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("");
        let ids = ids(host);
        if items.split(',').map(str::trim).filter(|i| !i.is_empty()).all(|i| ids.contains_key(i)) {
            out.push_str(rest[head_end..close].trim_start_matches('\n'));
        }
        rest = rest.get(close + "</IfMods>".len()..).unwrap_or("").trim_start_matches('\n');
    }
    out.push_str(rest);
    out
}

/// Item keys by id, made once.
fn ids(host: &dyn Host) -> &'static HashMap<String, ItemKey> {
    static IDS: OnceLock<HashMap<String, ItemKey>> = OnceLock::new();
    IDS.get_or_init(|| host.item_keys().into_iter().filter_map(|k| Some((k.id(host)?.to_owned(), k))).collect())
}

/// An item's name in the game's language (its tooltip's first line).
fn item_name(host: &dyn ClientHost, id: &str) -> String {
    static NAMES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    let mut g = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let names = g.get_or_insert_with(HashMap::new);
    if let Some(n) = names.get(id) {
        return n.clone();
    }
    let name = ids(host)
        .get(id)
        .and_then(|k| ItemResource::of(&ItemStack::of(*k)).tooltip(host, false, false))
        .and_then(|lines| lines.into_iter().find_map(|e| match e {
            TooltipElement::Text(t, _) => Some(host.text_string(&t)),
            _ => None,
        }))
        .unwrap_or_else(|| id.to_owned());
    names.insert(id.to_owned(), name.clone());
    name
}

fn tr(host: &dyn ClientHost, key: &str, fallback: &str) -> String {
    host.translate(key).unwrap_or_else(|| fallback.to_owned())
}

pub(crate) fn language(host: &dyn ClientHost) -> String {
    // The API has no "selected language" read: our own lang files name their language, then vanilla's key.
    host.translate("guideme.lang_code")
        .or_else(|| host.translate("language.code"))
        .unwrap_or_else(|| "en_us".to_owned())
        .to_lowercase()
}

/// Cuts the click places pushed since `first` to `area` (what a clip showed of them), dropping those wholly outside.
pub(crate) fn cut_targets(v: &mut View, first: usize, area: [f32; 4]) {
    let mut i = first;
    while i < v.targets.len() {
        match cut(v.targets[i].0, area) {
            Some(r) => {
                v.targets[i].0 = r;
                i += 1;
            }
            None => {
                v.targets.remove(i);
            }
        }
    }
}

/// The part of `r` inside `area`, None when they don't meet.
pub(crate) fn cut(r: [f32; 4], area: [f32; 4]) -> Option<[f32; 4]> {
    let (x0, y0) = (r[0].max(area[0]), r[1].max(area[1]));
    let (x1, y1) = ((r[0] + r[2]).min(area[0] + area[2]), (r[1] + r[3]).min(area[1] + area[3]));
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

/// Lays out styled words in a width, drawing the visible ones; returns the height used.
#[allow(clippy::too_many_arguments)]
/// A table cell's width on one line: its words (bold in a header) and its item links (icon and name).
fn cell_width(host: &dyn ClientHost, draw: &mut dyn DrawContext, cell: &[Span], header: bool) -> f32 {
    let space = draw.text_width(" ");
    let mut w = 0.0;
    for s in cell {
        if s.text == "\n" {
            continue;
        }
        if let Some(id) = s.text.strip_prefix('\u{1}') {
            w += 10.0 + draw.text_width(&item_name(host, id)) + space;
        } else {
            let shown = if s.bold || header { format!("§l{}", s.text) } else { s.text.clone() };
            w += draw.text_width(&shown);
        }
    }
    w
}

/// Table columns from their one-line widths: as wide as they need when the page has room (the rest shared out evenly),
/// else cut in proportion, never under a third of an even share so a short column stays readable.
pub(crate) fn column_widths(natural: &[f32], width: f32) -> Vec<f32> {
    let n = natural.len().max(1) as f32;
    let total: f32 = natural.iter().sum();
    if total <= width {
        let extra = (width - total) / n;
        return natural.iter().map(|w| w + extra).collect();
    }
    // Each column first gets its floor; the rest of the page goes to what columns need beyond it, in proportion.
    let floor = width / n / 3.0;
    let excess: Vec<f32> = natural.iter().map(|w| (w - floor).max(0.0)).collect();
    let spare = (width - floor * n).max(0.0);
    let sum: f32 = excess.iter().sum::<f32>().max(f32::EPSILON);
    excess.iter().map(|e| floor + e * spare / sum).collect()
}

fn flow(
    v: &mut View,
    host: &dyn ClientHost,
    draw: &mut dyn DrawContext,
    spans: &[Span],
    at: [f32; 2],
    width: f32,
    colour: u32,
    clip: [f32; 2],
) -> f32 {
    let (left, mut x, mut y) = (at[0], at[0], at[1]);
    let space = draw.text_width(" ");
    // Any part of the line on the page: the page's clip cuts the rest.
    let visible = |y: f32| y + LINE > clip[0] && y < clip[1];
    let g = v.guide;
    let page = v.page.clone();
    for s in spans {
        if s.text == "\n" {
            x = left;
            y += LINE;
            continue;
        }
        // An item link: its icon, then its name.
        if let Some(id) = s.text.strip_prefix('\u{1}') {
            let name = item_name(host, id);
            let w = 10.0 + draw.text_width(&name);
            if x > left && x + w > left + width {
                x = left;
                y += LINE;
            }
            if visible(y)
                && let Some(k) = ids(host).get(id)
            {
                draw.item(*k, [x, y - 1.0], 9.0);
            }
            if w <= width {
                if visible(y) {
                    draw.text(&name, [x + 10.0, y], LINK, false);
                    v.targets.push(([x, y - 1.0, w, LINE], Target::Item(id.to_owned())));
                }
                x += w + space;
                continue;
            }
            // Wider than the space (a narrow table column): the name wraps by words after the icon.
            let indent = x + 10.0;
            let mut wx = indent;
            for word in name.split(' ').filter(|w| !w.is_empty()) {
                let ww = draw.text_width(word);
                if wx > indent && wx + ww > left + width {
                    wx = indent;
                    y += LINE;
                }
                if visible(y) {
                    draw.text(word, [wx, y], LINK, false);
                    v.targets.push(([wx, y - 1.0, ww, LINE], Target::Item(id.to_owned())));
                }
                wx += ww + space;
            }
            x = wx;
            continue;
        }
        let words: Vec<&str> = s.text.split(' ').collect();
        let lead = s.text.starts_with(' ');
        for (wi, word) in words.iter().enumerate() {
            if word.is_empty() {
                if wi == 0 && lead && x > left {
                    x += space;
                }
                continue;
            }
            let shown = if s.bold { format!("§l{word}") } else if s.italic { format!("§o{word}") } else { (*word).to_owned() };
            let ww = draw.text_width(&shown);
            if x > left && x + ww > left + width {
                x = left;
                y += LINE;
            }
            let col = if s.link.is_some() { LINK } else if s.code { 0xFFB0_E0B0 } else if s.bold { 0xFFFF_FFFF } else { colour };
            if visible(y) {
                if s.code {
                    draw.fill([x - 1.0, y - 1.0, ww + 2.0, LINE], 0x40FF_FFFF);
                }
                draw.text(&shown, [x, y], col, false);
                match &s.link {
                    Some(Link::Page(p)) => {
                        let target = book::resolve(&page, p);
                        v.targets.push(([x, y - 1.0, ww, LINE], Target::Page(g, target)));
                    }
                    Some(Link::Item(id)) => v.targets.push(([x, y - 1.0, ww, LINE], Target::Item(id.clone()))),
                    _ => {}
                }
            }
            x += ww;
            if wi + 1 < words.len() {
                x += space;
            }
        }
    }
    y - at[1] + LINE
}

/// The recipe that makes `item` (or has id `recipe`), in vanilla's types and the guide's own.
/// The vanilla recipe types `<Recipe>` and `<RecipeFor>` look in. The server sends them to the clients
/// (`datapack_sync`): a client's own table has only vanilla's and content mods' recipes, not a native mod's
/// `data/` ones (Tinkers' `<Recipe id="tconstruct:common/fantastic_foundry">` drew nothing).
pub(crate) const VANILLA_KINDS: [&str; 8] = [
    "minecraft:crafting_shaped",
    "minecraft:crafting_shapeless",
    "minecraft:crafting_transmute",
    "minecraft:smelting",
    "minecraft:blasting",
    "minecraft:smoking",
    "minecraft:stonecutting",
    "minecraft:smithing_transform",
];

fn find_recipe(host: &dyn Host, guide: &Guide, item: Option<&str>, recipe: Option<&str>) -> Option<Recipe> {
    let mut kinds: Vec<String> = VANILLA_KINDS
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    kinds.extend(guide.recipe_types.iter().cloned());
    for kind in &kinds {
        for r in host.recipes_of_type(kind).iter() {
            if recipe.is_some_and(|id| r.id == id || r.id.ends_with(&format!(":{}", id.split(':').next_back().unwrap_or(id)))) {
                return Some(r.clone());
            }
            if let Some(item) = item {
                let made = r.results.first().and_then(|s| s.item().id(host).map(str::to_owned)).or_else(|| result_id(&r.json));
                if made.as_deref() == Some(item) {
                    return Some(r.clone());
                }
            }
        }
    }
    None
}

fn result_id(v: &Value) -> Option<String> {
    let r = &v["result"];
    r.as_str().or_else(|| r["id"].as_str()).or_else(|| r["item"].as_str()).map(|s| book::item_id("minecraft", s))
}

fn ingredient_items(host: &dyn Host, v: &Value) -> Vec<ItemKey> {
    let inner = v.get("ingredient").unwrap_or(v);
    match host.recipe_ingredient(&inner.to_string()) {
        Ok(Some(ing)) => ing.order.iter().filter_map(|id| ids(host).get(id).copied()).collect(),
        _ => Vec::new(),
    }
}

fn cycle(list: &[ItemKey], millis: u64) -> Option<ItemKey> {
    (!list.is_empty()).then(|| list[(millis / 1000) as usize % list.len()])
}

fn slot(draw: &mut dyn DrawContext, at: [f32; 2]) {
    draw.fill([at[0], at[1], 18.0, 18.0], 0xFF37_3737);
    draw.fill([at[0] + 1.0, at[1] + 1.0, 16.0, 16.0], 0xFF8B_8B8B);
}

/// A recipe as a grid of inputs, an arrow and its result; returns its height.
#[allow(clippy::too_many_arguments)]
fn draw_recipe(v: &mut View, host: &dyn ClientHost, draw: &mut dyn DrawContext, r: &Recipe, at: [f32; 2], show: bool, millis: u64, layout: Option<&Value>) -> f32 {
    if !VANILLA_KINDS.contains(&r.kind.as_str()) {
        return draw_mod_recipe(v, host, draw, r, at, show, millis, layout);
    }
    let json = &*r.json;
    let mut cells: Vec<(usize, usize, Vec<ItemKey>)> = Vec::new();
    if r.kind == "minecraft:crafting_shaped" {
        let pattern: Vec<&str> = json["pattern"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        for (y, row) in pattern.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if ch != ' ' {
                    cells.push((x, y, ingredient_items(host, &json["key"][ch.to_string()])));
                }
            }
        }
    } else {
        let list: Vec<Vec<ItemKey>> = if !r.ingredients.is_empty() {
            r.ingredients.iter().map(|i| i.order.iter().filter_map(|id| ids(host).get(id).copied()).collect()).collect()
        } else {
            ["ingredient", "input", "template", "base", "addition", "material"]
                .iter()
                .filter(|k| !json[**k].is_null())
                .map(|k| ingredient_items(host, &json[*k]))
                .chain(json["ingredients"].as_array().into_iter().flatten().map(|i| ingredient_items(host, i)))
                .collect()
        };
        let crafting = r.kind.starts_with("minecraft:crafting");
        let side = if crafting { if list.len() <= 4 { 2 } else { 3 } } else { list.len().max(1) };
        for (i, items) in list.into_iter().enumerate() {
            cells.push((i % side, i / side, items));
        }
    }
    let rows = cells.iter().map(|c| c.1 + 1).max().unwrap_or(1).max(1);
    let cols = cells.iter().map(|c| c.0 + 1).max().unwrap_or(1).max(1);
    let h = rows as f32 * 18.0;
    if show {
        for (x, y, items) in &cells {
            let p = [at[0] + *x as f32 * 18.0, at[1] + *y as f32 * 18.0];
            slot(draw, p);
            if let Some(k) = cycle(items, millis) {
                draw.item(k, [p[0] + 1.0, p[1] + 1.0], 16.0);
                v.targets.push(([p[0], p[1], 18.0, 18.0], Target::Item(k.id(host).unwrap_or_default().to_owned())));
            }
        }
        let ax = at[0] + cols as f32 * 18.0 + 6.0;
        let ay = at[1] + (h - 9.0) / 2.0;
        draw.text("->", [ax, ay], TEXT, false);
        let rp = [ax + 18.0, at[1] + (h - 18.0) / 2.0];
        slot(draw, rp);
        let result = r.results.first().cloned().or_else(|| result_id(json).and_then(|id| ids(host).get(&id).and_then(|k| ItemStack::new(*k, json["result"]["count"].as_u64().unwrap_or(1) as u32))));
        if let Some(stack) = result {
            draw.gui_draw_item_with(&stack, [rp[0] + 1.0, rp[1] + 1.0], None);
            v.targets.push(([rp[0], rp[1], 18.0, 18.0], Target::Item(stack.item().id(host).unwrap_or_default().to_owned())));
        }
    }
    h + 6.0
}

/// One slot of a mod's recipe: items to cycle through (with a count), or a fluid and its amount (mB).
enum Cell {
    Items(Vec<ItemKey>, u32),
    Fluid(String, u64),
}

/// `a.0.b`: a key path into a recipe's JSON.
fn at_path<'a>(v: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(v, |v, k| match k.parse::<usize>() {
        Ok(i) if v.is_array() => &v[i],
        _ => &v[k],
    })
}

/// A JSON value of a mod's recipe as slots: a fluid (`{"fluid": id, "amount": n}`, a fluid `tag` with an amount),
/// an item (`"id"`, `{"item"|"id": id, "count": n}`), an ingredient (a `#tag`, a list), or a list of them.
fn cells_of(host: &dyn Host, ns: &str, v: &Value, out: &mut Vec<Cell>) {
    if v.is_null() {
        return;
    }
    if let Some(a) = v.as_array()
        && a.iter().all(Value::is_object)
        && a.iter().any(|x| !x["amount"].is_null() || !x["fluid"].is_null() || !x["item"].is_null() || !x["count"].is_null())
    {
        for x in a {
            cells_of(host, ns, x, out);
        }
        return;
    }
    let amount = v["amount"].as_u64();
    if let Some(f) = v["fluid"].as_str().or_else(|| v["fluid"]["fluid"].as_str()) {
        out.push(Cell::Fluid(book::item_id("minecraft", f), amount.or_else(|| v["fluid"]["amount"].as_u64()).unwrap_or(0)));
        return;
    }
    if let (Some(tag), Some(n)) = (v["tag"].as_str(), amount) {
        // A fluid tag (`c:molten_iron`): the guide's own fluid of that name.
        let name = tag.rsplit([':', '/']).next().unwrap_or(tag);
        out.push(Cell::Fluid(format!("{ns}:{name}"), n));
        return;
    }
    // Wrappers the game can't read: Mantle's fluid container shows its `display` item, Forge's difference its `base`.
    for k in ["display", "base"] {
        if v[k].is_object() {
            return cells_of(host, ns, &v[k], out);
        }
    }
    let count = v["count"].as_u64().unwrap_or(1) as u32;
    // An item tag in the old `{"tag": t}` form: the game reads `#t`.
    if let Some(tag) = v["tag"].as_str() {
        // Forge's tags are the common `c:` ones here.
        let mut items = ingredient_items(host, &Value::String(format!("#{tag}")));
        if items.is_empty() && let Some(rest) = tag.strip_prefix("forge:") {
            items = ingredient_items(host, &Value::String(format!("#c:{rest}")));
        }
        if !items.is_empty() {
            out.push(Cell::Items(items, count));
        }
        return;
    }
    let id = v.as_str().filter(|s| !s.starts_with('#')).or_else(|| v["item"].as_str()).or_else(|| v["id"].as_str());
    let items = match id.and_then(|i| ids(host).get(&book::item_id("minecraft", i)).copied()) {
        Some(k) => vec![k],
        None => ingredient_items(host, v),
    };
    if !items.is_empty() {
        out.push(Cell::Items(items, count));
    }
}

fn fluid_tip(host: &dyn ClientHost, id: &str, mb: u64) -> String {
    let name = host.fluid_name(id);
    if mb == 0 { name } else { format!("{name}\n§7{} B", trim_num(mb as f64 / 1000.0)) }
}

fn trim_num(x: f64) -> String {
    let s = format!("{x:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A mod's own recipe type: its inputs, an arrow (after the machine that makes it, when the guide names one), and its
/// outputs, from the guide's layout (`guide.json` "recipe_layouts") or the JSON's usual keys; returns its height.
#[allow(clippy::too_many_arguments)]
fn draw_mod_recipe(v: &mut View, host: &dyn ClientHost, draw: &mut dyn DrawContext, r: &Recipe, at: [f32; 2], show: bool, millis: u64, layout: Option<&Value>) -> f32 {
    let json = &*r.json;
    let ns = r.kind.split(':').next().unwrap_or("minecraft");
    let keys = |name: &str, default: &[&str]| -> Vec<String> {
        match layout.and_then(|l| l[name].as_array()) {
            Some(a) => a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
            None => default.iter().map(|s| (*s).to_owned()).collect(),
        }
    };
    let mut inputs = Vec::new();
    // A plural key (`inputs`, `ingredients`, `outputs`) is a list of slots; a singular one is one slot, a list there
    // being an ingredient's alternatives.
    let each = |k: &str, out: &mut Vec<Cell>| {
        let val = at_path(json, k);
        match val.as_array() {
            Some(a) if k.ends_with('s') => a.iter().for_each(|x| cells_of(host, ns, x, out)),
            _ => cells_of(host, ns, val, out),
        }
    };
    for k in keys("inputs", &["cast", "ingredient", "input", "inputs", "ingredients", "fluid", "fluids", "catalyst"]) {
        each(&k, &mut inputs);
    }
    if inputs.is_empty() {
        for i in &r.ingredients {
            inputs.push(Cell::Items(i.order.iter().filter_map(|id| ids(host).get(id).copied()).collect(), 1));
        }
    }
    let mut outputs = Vec::new();
    for k in keys("outputs", &["result", "results", "output", "outputs", "byproducts"]) {
        each(&k, &mut outputs);
    }
    if outputs.is_empty() {
        outputs.extend(r.results.iter().map(|s| Cell::Items(vec![s.item()], s.count())));
    }
    let label = layout.and_then(|l| l["label"].as_str()).map(|k| tr(host, k, k));
    let machine = layout.and_then(|l| l["machine"].as_str()).and_then(|m| ids(host).get(&book::item_id(ns, m)).copied());
    let top = if label.is_some() { LINE + 2.0 } else { 0.0 };
    let cols = 3usize;
    let rows = inputs.len().div_ceil(cols).max(outputs.len().div_ceil(2)).max(1);
    let h = top + rows as f32 * 18.0;
    if show {
        if let Some(l) = &label {
            draw.text(l, at, DIM, false);
        }
        let y0 = at[1] + top;
        let put = |v: &mut View, draw: &mut dyn DrawContext, c: &Cell, p: [f32; 2]| {
            slot(draw, p);
            match c {
                Cell::Items(items, n) => {
                    if let Some(k) = cycle(items, millis) {
                        match ItemStack::new(k, *n).filter(|_| *n > 1) {
                            Some(st) => draw.gui_draw_item_with(&st, [p[0] + 1.0, p[1] + 1.0], None),
                            None => {
                                draw.item(k, [p[0] + 1.0, p[1] + 1.0], 16.0);
                            }
                        }
                        v.targets.push(([p[0], p[1], 18.0, 18.0], Target::Item(k.id(host).unwrap_or_default().to_owned())));
                    }
                }
                Cell::Fluid(f, mb) => {
                    draw.fluid(f, [p[0] + 1.0, p[1] + 1.0], 16.0);
                    v.targets.push(([p[0], p[1], 18.0, 18.0], Target::Tip(fluid_tip(host, f, *mb))));
                }
            }
        };
        for (i, c) in inputs.iter().enumerate() {
            put(v, draw, c, [at[0] + (i % cols) as f32 * 18.0, y0 + (i / cols) as f32 * 18.0]);
        }
        let ax = at[0] + inputs.len().clamp(1, cols) as f32 * 18.0 + 6.0;
        let mid = y0 + (rows as f32 * 18.0 - 18.0) / 2.0;
        if let Some(k) = machine {
            draw.item(k, [ax, mid + 1.0], 16.0);
            v.targets.push(([ax, mid + 1.0, 16.0, 16.0], Target::Item(k.id(host).unwrap_or_default().to_owned())));
        }
        let arrow = ax + if machine.is_some() { 20.0 } else { 0.0 };
        draw.text("->", [arrow, mid + 5.0], TEXT, false);
        let ox = arrow + 18.0;
        for (i, c) in outputs.iter().enumerate() {
            put(v, draw, c, [ox + (i % 2) as f32 * 18.0, y0 + (i / 2) as f32 * 18.0]);
        }
    }
    h + 6.0
}

/// The block entity a scene's `<Block entity=... nbt=...>` asks for, its fields read from the SNBT.
fn scene_entity(s: &md::Scene, pos: [i32; 3]) -> Option<SceneBlockEntity> {
    let (_, ty, nbt) = s.entities.iter().find(|e| e.0 == pos)?;
    let Some(crate::snbt::Tag::Compound(c)) = crate::snbt::parse(nbt) else { return None };
    let fields = mod_api::item::containers::ModBlockEntityFields { nbt: compound(&c), ..Default::default() };
    Some(SceneBlockEntity::new(Identifier::parse(ty).ok()?, fields))
}

use mod_api::JavaString;

fn compound(c: &[(String, crate::snbt::Tag)]) -> Compound {
    let mut out = Compound::new();
    for (k, v) in c {
        out.insert(JavaString::from(k.as_str()), nbt_tag(v));
    }
    out
}

fn nbt_tag(t: &crate::snbt::Tag) -> Tag {
    use crate::snbt::Tag as S;
    match t {
        S::Compound(c) => Tag::Compound(compound(c)),
        S::List(l) => Tag::List(l.iter().map(nbt_tag).collect()),
        S::Str(s) => Tag::String(JavaString::from(s.as_str())),
        S::Num(n) if n.fract() == 0.0 && n.abs() < 2.0e9 => Tag::Int(*n as i32),
        S::Num(n) => Tag::Double(n.to_bits()),
    }
}

fn scene_blocks(guide: &Guide, page: &str, s: &md::Scene) -> Vec<([i32; 3], String)> {
    let mut out = s.blocks.clone();
    for src in &s.structures {
        let path = book::resolve(page, src);
        let found = guide.scenes.get(&path).or_else(|| guide.scenes.get(&path.replace(".snbt", ".json")));
        out.extend(found.cloned().unwrap_or_default());
    }
    out
}

pub(crate) fn draw(host: &dyn ClientHost, draw: &mut dyn DrawContext, mouse: [f32; 2], millis: u64) {
    let hover = with(|v| {
        if !v.open {
            return None;
        }
        v.targets.clear();
        if v.lang.is_empty() {
            v.lang = language(host);
        }
        with_guides(|guides| {
            let Some(guide) = guides.get(v.guide) else {
                v.open = false;
                return;
            };
            render(v, host, draw, guide, mouse, millis);
        });
        v.hover.take()
    });
    // No lock held from here: the tooltip may call back into this mod (item_tooltip, `api`).
    match hover {
        Some(Target::Item(id)) => {
            let Some(k) = ids(host).get(&id) else { return };
            let mut lines: Vec<Text> = ItemResource::of(&ItemStack::of(*k))
                .tooltip(host, false, false)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|e| match e {
                    TooltipElement::Text(t, _) => Some(t),
                    _ => None,
                })
                .collect();
            if book::page_of_item(&id).is_some() {
                lines.push(Text::literal(format!("§7{}", tr(host, "guideme.click_to_open", "Click to open its page"))));
            }
            let _ = draw.gui_tooltip(&lines, mouse);
        }
        Some(Target::Tip(t)) => {
            let lines: Vec<Text> = t.lines().map(Text::literal).collect();
            let _ = draw.gui_tooltip(&lines, mouse);
        }
        Some(Target::Scene(_)) => {
            let _ = draw.gui_tooltip(&[Text::literal(format!("§7{}", tr(host, "guideme.scene_hint", "Drag to turn, scroll to zoom")))], mouse);
        }
        _ => {}
    }
}

/// Whether the guide is open, without waiting: the lock held means the guide's own draw is asking (a tooltip), so
/// it is open. For hooks the game may call while the guide draws.
pub(crate) fn open_or_busy() -> bool {
    match VIEW.try_lock() {
        Ok(g) => g.as_ref().is_some_and(|v| v.open),
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner().as_ref().is_some_and(|v| v.open),
        Err(std::sync::TryLockError::WouldBlock) => true,
    }
}

fn render(v: &mut View, host: &dyn ClientHost, draw: &mut dyn DrawContext, guide: &Guide, mouse: [f32; 2], millis: u64) {
    let [sw, sh] = draw.size();
    let (w, h) = ((sw - 16.0).min(480.0), (sh - 16.0).min(320.0));
    let (x0, y0) = (((sw - w) / 2.0).floor(), ((sh - h) / 2.0).floor());
    v.rect = [x0, y0, w, h];
    // Twice: one fill over a screen's slots still lets them show through.
    draw.fill([0.0, 0.0, sw, sh], 0xA000_0000);
    draw.fill([x0 - 1.0, y0 - 1.0, w + 2.0, h + 2.0], EDGE);
    draw.fill([x0, y0, w, h], BG);
    draw.fill([x0, y0, w, h], BG);
    // Title bar: the guide's name, then Back, Contents and the cross on the right.
    // guide.json's "title": a translation key, or the name itself (no key-like dots).
    let fallback = if guide.title.contains('.') { format!("{} Guide", guide.ns) } else { guide.title.clone() };
    let title = tr(host, &guide.title, &fallback);
    draw.text(&title, [x0 + 6.0, y0 + 5.0], HEAD, true);
    let close = [x0 + w - 14.0, y0 + 3.0, 11.0, 11.0];
    draw.text("x", [close[0] + 3.0, close[1] + 1.0], if inside(close, mouse) { 0xFFFF_FFFF } else { DIM }, false);
    v.targets.push((close, Target::Close));
    let contents = tr(host, "guideme.contents", "Contents");
    let back = format!("< {}", tr(host, "gui.back", "Back"));
    let cw = draw.text_width(&contents);
    let bw = draw.text_width(&back);
    let cr = [close[0] - cw - 10.0, y0 + 3.0, cw + 4.0, 11.0];
    let br = [cr[0] - bw - 10.0, y0 + 3.0, bw + 4.0, 11.0];
    for (r, t, target, enabled) in [(cr, &contents, Target::Contents, true), (br, &back, Target::Back, !v.history.is_empty())] {
        let col = if !enabled { 0xFF50_5060 } else if inside(r, mouse) { 0xFFFF_FFFF } else { DIM };
        draw.text(t, [r[0] + 2.0, r[1] + 1.0], col, false);
        if enabled {
            v.targets.push((r, target));
        }
    }
    draw.fill([x0, y0 + 16.0, w, 1.0], EDGE);
    // Navigation: the page tree, current page highlighted, scrolled on its own.
    let nav = [x0, y0 + 17.0, NAV_W, h - 17.0];
    v.nav = nav;
    draw.fill(nav, NAV_BG);
    draw.fill([x0 + NAV_W, y0 + 17.0, 1.0, h - 17.0], EDGE);
    let mut rows: Vec<(usize, String)> = Vec::new();
    fn walk(g: &Guide, parent: Option<&str>, depth: usize, rows: &mut Vec<(usize, String)>) {
        for c in g.children(parent) {
            rows.push((depth, c.clone()));
            if depth < 6 {
                walk(g, Some(c), depth + 1, rows);
            }
        }
    }
    walk(guide, None, 0, &mut rows);
    // The panel's own target first: the rows drawn over it are found before it.
    v.targets.push((nav, Target::Nav));
    let mut y = nav[1] + 4.0 - v.nav_scroll;
    // The tree is cut to its panel (GuideME's scissor), so a row scrolled half out shows its half.
    let nav_clipped = draw.push_clip(nav).is_ok();
    let nav_first = v.targets.len();
    for (depth, path) in &rows {
        let Some(p) = guide.page(path, &v.lang) else { continue };
        if !p.requires.iter().all(|i| ids(host).contains_key(i.as_str())) {
            continue;
        }
        let r = [nav[0] + 2.0, y, NAV_W - 4.0, 12.0];
        if y + 12.0 > nav[1] && y < nav[1] + nav[3] {
            if *path == v.page {
                draw.fill(r, 0x6050_70C0);
            } else if inside(r, mouse) {
                draw.fill(r, 0x3080_80A0);
            }
            let ix = r[0] + 2.0 + *depth as f32 * 6.0;
            let mut tx = ix;
            if let Some(icon) = p.nav.icon.as_ref().and_then(|i| ids(host).get(&book::item_id(&guide.ns, i))) {
                draw.item(*icon, [ix, y + 1.5], 9.0);
                tx += 11.0;
            }
            let mut label = p.nav.title.clone();
            while draw.text_width(&label) > r[0] + r[2] - tx && label.chars().count() > 3 {
                label.pop();
            }
            draw.text(&label, [tx, y + 2.0], if *path == v.page { 0xFFFF_FFFF } else { TEXT }, false);
            v.targets.push((r, Target::Page(v.guide, path.clone())));
        }
        y += 12.0;
    }
    if nav_clipped {
        let _ = draw.pop_clip();
    }
    cut_targets(v, nav_first, nav);
    v.nav_height = y + v.nav_scroll - nav[1];
    // The page.
    let content = [x0 + NAV_W + 8.0, y0 + 20.0, w - NAV_W - 16.0, h - 24.0];
    v.content = content;
    let clip = [content[1], content[1] + content[3]];
    let Some(page) = guide.page(&v.page, &v.lang).cloned() else {
        draw.text(&format!("{}?", v.page), [content[0], content[1]], TEXT, false);
        return;
    };
    let body = if_mods(host, &page.body);
    let blocks = md::blocks(&body, &guide.ns);
    let (left, width) = (content[0], content[2]);
    let mut y = content[1] - v.scroll;
    let mut scene_n = 0;
    // The page is cut to its panel (GuideME's scroll area scissor): what is partly on it shows its part, and the
    // places that take clicks are cut to match.
    let page_clipped = draw.push_clip(content).is_ok();
    let page_first = v.targets.len();
    // A commands page says who may run it: "Everyone", or the operator level the server checks (the menu only lists
    // what the player's level allows).
    if guide.commands {
        let who = match page.level {
            0 => tr(host, "guideme.commands.everyone", "Everyone can run this"),
            n => tr(host, "guideme.commands.level", "Operator level %s").replacen("%s", &n.to_string(), 1),
        };
        if y + LINE > clip[0] && y < clip[1] {
            let tw = draw.text_width(&who);
            draw.text(&who, [left + width - tw, y], DIM, false);
        }
        y += LINE + 2.0;
    }
    for b in &blocks {
        match b {
            Block::Heading(level, spans) => {
                y += if *level <= 1 { 2.0 } else { 4.0 };
                let spans: Vec<Span> = spans.iter().map(|s| Span { bold: true, ..s.clone() }).collect();
                y += flow(v, host, draw, &spans, [left, y], width, HEAD, clip) + 3.0;
                if *level <= 2 && y - 1.0 > clip[0] && y - 2.0 < clip[1] {
                    draw.fill([left, y - 2.0, width, 1.0], 0x60FF_E080);
                }
                y += 2.0;
            }
            Block::Para(spans) => y += flow(v, host, draw, spans, [left, y], width, TEXT, clip) + 4.0,
            Block::Quote(spans) => {
                let top = y;
                y += flow(v, host, draw, spans, [left + 8.0, y], width - 8.0, DIM, clip);
                if y > clip[0] && top - 1.0 < clip[1] {
                    draw.fill([left + 2.0, top - 1.0, 2.0, y - top], DIM);
                }
                y += 4.0;
            }
            Block::Item(depth, n, spans) => {
                let indent = 10.0 + *depth as f32 * 10.0;
                let mark = n.map(|n| format!("{n}.")).unwrap_or_else(|| "•".to_owned());
                if y + LINE > clip[0] && y < clip[1] {
                    draw.text(&mark, [left + indent - 9.0, y], DIM, false);
                }
                y += flow(v, host, draw, spans, [left + indent, y], width - indent, TEXT, clip) + 1.0;
            }
            Block::Code(code) => {
                for line in code.lines() {
                    if y + LINE > clip[0] && y < clip[1] {
                        draw.fill([left, y - 1.0, width, LINE], 0x40000000);
                        draw.text(line, [left + 2.0, y], 0xFFB0_E0B0, false);
                    }
                    y += LINE;
                }
                y += 4.0;
            }
            Block::Rule => {
                if y >= clip[0] && y < clip[1] {
                    draw.fill([left, y + 3.0, width, 1.0], EDGE);
                }
                y += 8.0;
            }
            Block::Table(rows) => {
                let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                // Each column as wide as its widest cell on one line (a header bold), then fitted to the page.
                let mut natural = vec![0.0f32; cols];
                for (ri, row) in rows.iter().enumerate() {
                    for (ci, cell) in row.iter().enumerate() {
                        natural[ci] = natural[ci].max(cell_width(host, draw, cell, ri == 0) + 4.0);
                    }
                }
                let widths = column_widths(&natural, width);
                let mut starts = Vec::with_capacity(cols);
                let mut at = left;
                for w in &widths {
                    starts.push(at);
                    at += w;
                }
                for (ri, row) in rows.iter().enumerate() {
                    let mut tallest: f32 = LINE;
                    for (ci, cell) in row.iter().enumerate() {
                        let spans: Vec<Span> = cell.iter().map(|s| Span { bold: s.bold || ri == 0, ..s.clone() }).collect();
                        tallest = tallest.max(flow(v, host, draw, &spans, [starts[ci] + 2.0, y], widths[ci] - 4.0, TEXT, clip));
                    }
                    y += tallest + 2.0;
                    if y > clip[0] && y < clip[1] {
                        draw.fill([left, y - 1.0, width, 1.0], 0x30FF_FFFF);
                    }
                }
                y += 4.0;
            }
            Block::Items(items) => {
                let mut x = left;
                let mut tallest: f32 = 0.0;
                for (id, scale) in items {
                    let size = (16.0 * scale.clamp(1.0, 4.0)).min(64.0);
                    if x + size > left + width && x > left {
                        x = left;
                        y += tallest + 4.0;
                        tallest = 0.0;
                    }
                    if y + size > clip[0] && y < clip[1]
                        && let Some(k) = ids(host).get(id)
                    {
                        draw.item(*k, [x, y], size);
                        v.targets.push(([x, y, size, size], Target::Item(id.clone())));
                    }
                    x += size + 6.0;
                    tallest = tallest.max(size);
                }
                y += tallest + 6.0;
            }
            Block::RecipeFor(id) | Block::Recipe(id) => {
                let (item, recipe) = if matches!(b, Block::RecipeFor(_)) { (Some(id.as_str()), None) } else { (None, Some(id.as_str())) };
                if let Some(r) = find_recipe(host, guide, item, recipe) {
                    let layout = guide.layouts.get(&r.kind);
                    let hh = draw_recipe(v, host, draw, &r, [left, y], false, millis, layout);
                    let show = y + hh > clip[0] && y < clip[1];
                    draw_recipe(v, host, draw, &r, [left, y], show, millis, layout);
                    y += hh;
                }
            }
            Block::Scene(s) => {
                let cells = scene_blocks(guide, &v.page, s);
                let mut s = s.clone();
                for src in &s.structures.clone() {
                    let path = book::resolve(&v.page, src);
                    let found = guide.scene_entities.get(&path).or_else(|| guide.scene_entities.get(&path.replace(".snbt", ".json")));
                    s.entities.extend(found.cloned().unwrap_or_default());
                }
                let s = &s;
                let idx = scene_n;
                scene_n += 1;
                if cells.is_empty() {
                    continue;
                }
                let (lo, hi) = cells.iter().fold(([i32::MAX; 3], [i32::MIN; 3]), |(lo, hi), (p, _)| {
                    (std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i] + 1)))
                });
                let extent = (0..3).map(|i| (hi[i] - lo[i]) as f32).fold(1.0, f32::max);
                let size = (width * 0.8).min(150.0).max(80.0);
                let rect = [left + (width - size) / 2.0, y, size, size];
                // Drawn while any of it is on the page, cut to the page's area, so it slides in and out with the scroll.
                if y + size > clip[0] && y < clip[1] {
                    let (yaw, pitch, zoom) = *v.scenes.entry(idx).or_insert((s.yaw, s.pitch, 1.0));
                    let center: [f32; 3] = std::array::from_fn(|i| (lo[i] + hi[i]) as f32 / 2.0);
                    let blocks: Vec<SceneBlock> = cells
                        .iter()
                        // A state whose properties this port's block lacks (AE2's formed rings): the block's default state.
                        .filter_map(|(p, id)| {
                            host.block_state_id(id)
                                .or_else(|| host.block_state_id(id.split('[').next().unwrap_or(id)))
                                .map(|st| SceneBlock::new(*p, st, scene_entity(s, *p)))
                        })
                        .collect();
                    let scene = mod_api::Scene::new(blocks);
                    // An isometric view spans about 1.4 blocks per block of the scene's longest side.
                    let scale = size / extent / 1.45 * zoom;
                    // The whole box: the page's clip cuts what is scrolled off (the scene keeps its depth).
                    draw.fill(rect, 0x3000_0000);
                    // The scene's box is its corners (x0, y0, x1, y1), not a size.
                    let _ = draw.draw_scene([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]], &scene, SceneView::new(center, yaw, pitch, scale));
                    if scene.blocks.is_empty() {
                        draw.text("?", [rect[0] + 2.0, rect[1] + 2.0], DIM, false);
                    }
                    v.targets.push((rect, Target::Scene(idx)));
                }
                y += size + 6.0;
            }
            Block::SubPages(_) | Block::CategoryIndex(_) => {
                // The pages under this one, or in a category, as links with their icons.
                let list: Vec<String> = match b {
                    Block::CategoryIndex(cat) => {
                        let mut l: Vec<(String, String)> = guide
                            .pages
                            .iter()
                            .filter(|(_, p)| p.categories.iter().any(|c| c == cat))
                            .map(|(k, p)| (p.nav.title.clone(), k.clone()))
                            .collect();
                        l.sort();
                        l.into_iter().map(|(_, k)| k).collect()
                    }
                    _ => guide.children(Some(&v.page)).into_iter().cloned().collect(),
                };
                for path in list {
                    let Some(p) = guide.page(&path, &v.lang) else { continue };
                    if y + LINE > clip[0] && y < clip[1] {
                        if let Some(icon) = p.nav.icon.as_ref().and_then(|i| ids(host).get(&book::item_id(&guide.ns, i))) {
                            draw.item(*icon, [left + 4.0, y - 1.0], 9.0);
                        }
                        draw.text(&p.nav.title, [left + 16.0, y], LINK, false);
                        let tw = draw.text_width(&p.nav.title);
                        v.targets.push(([left, y - 1.0, tw + 16.0, LINE], Target::Page(v.guide, path.clone())));
                    }
                    y += LINE + 1.0;
                }
                y += 4.0;
            }
            Block::Image(alt, src) => {
                // The PNG, from the GUI atlas (see `image_sprite`); its alt text when no sprite has it.
                if let Some((name, size)) = image_sprite(host, &guide.ns, &v.page, src) {
                    let scale = (width / size[0] as f32).min(1.0);
                    let (w, h) = (size[0] as f32 * scale, size[1] as f32 * scale);
                    if y + h > clip[0] && y < clip[1] {
                        draw.sprite(&name, [0.0, 0.0, size[0] as f32, size[1] as f32], [left + (width - w) / 2.0, y, w, h], None);
                    }
                    y += h + 6.0;
                } else {
                    if y + LINE > clip[0] && y < clip[1] {
                        draw.text(&format!("[{alt}]"), [left, y], DIM, false);
                    }
                    y += LINE + 4.0;
                }
            }
        }
    }
    if page_clipped {
        let _ = draw.pop_clip();
    }
    cut_targets(v, page_first, content);
    v.page_height = y + v.scroll - content[1];
    // A scroll bar when the page is longer than the panel.
    if v.page_height > content[3] {
        let track = [x0 + w - 4.0, content[1], 2.0, content[3]];
        draw.fill(track, 0x40FF_FFFF);
        let thumb = (content[3] * content[3] / v.page_height).max(10.0);
        let at = content[1] + (content[3] - thumb) * (v.scroll / (v.page_height - content[3]).max(1.0));
        draw.fill([track[0], at, 2.0, thumb], 0xC0FF_FFFF);
    }
    // The tooltip under the pointer is drawn by `draw` once the guide's locks are released: an item's tooltip runs
    // every mod's tooltip hook (this one's too), which would wait forever for the locks this frame holds.
    v.hover = v.target_at(mouse);
}

/// A page picture's GUI sprite and size. `src` is read as GuideME reads it (relative to the page, `/` from the
/// guide's root, `ns:path` from that guide's root); the PNG is looked up in the GUI atlas as
/// `<ns>:gui/ae2guide/<path>` (the same file under `assets/<ns>/textures/gui/ae2guide/`), then as
/// `<ns>:gui/guide/<path>` (where our upstream tools put the pictures). Sunrise's GUI atlas reads only
/// `textures/`, so a PNG left in `ae2guide/` alone is not found: the alt text shows instead.
fn image_sprite(host: &dyn ClientHost, ns: &str, page: &str, src: &str) -> Option<(String, [u32; 2])> {
    if src.is_empty() || src.contains("://") {
        return None;
    }
    let (ns, path) = match src.split_once(':') {
        Some((n, p)) => (n.to_owned(), p.trim_start_matches('/').to_owned()),
        None => (ns.to_owned(), book::resolve(page, src)),
    };
    let path = path.strip_suffix(".png").unwrap_or(&path).to_owned();
    for dir in ["gui/ae2guide/", "gui/guide/"] {
        let name = format!("{ns}:{dir}{path}");
        let Ok(id) = Identifier::from_parts(&ns, &format!("{dir}{path}")) else { continue };
        if let Some(r) = host.gui_sprite(&id)
            && r.size[0] > 0
        {
            return Some((name, r.size));
        }
    }
    None
}

/// A click: true when the guide took it (every click while it is open).
pub(crate) fn click(at: [f32; 2], button: i32) -> bool {
    with(|v| {
        if !v.open {
            return false;
        }
        match v.target_at(at) {
            Some(Target::Close) => v.close(),
            Some(Target::Back) => v.back(),
            Some(Target::Contents) => {
                let start = with_guides(|g| g.get(v.guide).map(|g| g.start.clone()));
                if let Some(s) = start {
                    let g = v.guide;
                    v.go(g, s);
                }
            }
            Some(Target::Page(g, p)) => {
                if p != v.page || g != v.guide {
                    v.go(g, p);
                }
            }
            Some(Target::Item(id)) => {
                if let Some((g, p)) = book::page_of_item(&id)
                    && (p != v.page || g != v.guide)
                {
                    v.go(g, p);
                }
            }
            Some(Target::Scene(i)) => {
                if button == 0 {
                    v.dragging = Some(i);
                }
            }
            Some(Target::Tip(_)) => {}
            Some(Target::Nav) | None => {
                if !inside(v.rect, at) && !v.own_screen {
                    v.close();
                }
            }
        }
        true
    })
}

pub(crate) fn drag(delta: [f32; 2]) -> bool {
    with(|v| {
        let Some(i) = v.dragging else { return v.open };
        if let Some(s) = v.scenes.get_mut(&i) {
            s.0 += delta[0] * 1.5;
            s.1 = (s.1 + delta[1] * 1.5).clamp(-89.0, 89.0);
        }
        true
    })
}

pub(crate) fn release() -> bool {
    with(|v| {
        v.dragging = None;
        v.open
    })
}

pub(crate) fn scroll(at: [f32; 2], dy: f32) -> bool {
    with(|v| {
        if !v.open {
            return false;
        }
        match v.target_at(at) {
            Some(Target::Scene(i)) => {
                if let Some(s) = v.scenes.get_mut(&i) {
                    s.2 = (s.2 * if dy > 0.0 { 1.15 } else { 1.0 / 1.15 }).clamp(0.3, 5.0);
                }
            }
            _ if inside(v.nav, at) => {
                v.nav_scroll = (v.nav_scroll - dy * 24.0).clamp(0.0, (v.nav_height - v.nav[3]).max(0.0));
            }
            _ => {
                v.scroll = (v.scroll - dy * 24.0).clamp(0.0, (v.page_height - v.content[3]).max(0.0));
            }
        }
        true
    })
}
