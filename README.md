# GuideME for Sunrise (the W guide)

AE2's guidebook (LGPL-3.0; see `LICENSE`, `NOTICE`) as **one shared library mod** for [Sunrise](https://playsunrise.net)
(Minecraft Java 26.3), mod id `guideme`, version 0.1.0, client side. Applied Energistics 2, Iron Furnaces,
Hostile Neural Networks, Tinkers' Construct, JEI and Jade for Sunrise read their guides through it.

## Adding a guide to a mod: files only

1. Put the pages in **`assets/<your namespace>/ae2guide/`** (assets, not data: the client reads them with a client
   reload listener; Sunrise gives a client no data files). Upstream ports have them in `data/<ns>/ae2guide/`:
   move or copy the folder, unchanged.
2. Optional: in `sunrise-mod.toml`, `[dependencies] guideme = { version = "^0.1", optional = true }`, so the
   launcher orders it and players see the link. Nothing in `Cargo.toml`, no code.
3. Optional: `ae2guide/guide.json`: `{"title": "<ns>.guide.title", "start": "index.md", "recipe_types":
   ["<ns>:<type>"]}` (title is a translation key or text; `recipe_types` are more types `<RecipeFor>` looks in).
   Without it the title is the key `<ns>.guide.title`, the start page `index.md`.

What is read (the same as AE2's guide and our upstream tools write):
- **Pages**: `*.md` with YAML front matter: `navigation: {title, parent, position, icon}`, `item_ids: [..]`
  (W over those items opens the page), `categories: [..]`.
- **`index.json`** (our tools' index: `items`, `titles`, `titles_<lang>`, `parents`, `positions`, `icons`,
  `requires`) fills what the front matter leaves out.
- **Other mods**: a page's `requires: [..]` (front matter or index; an item of each other mod it is about) keeps it
  out of the navigation unless they are all in the game, and `<IfMods items="a:x,b:y">...</IfMods>` shows its part
  only then (the tags never show).
- **Translations**: `_<lang>/<same path>.md` (AE2's layout, e.g. `_es_es/index.md`), chosen by the game's language
  (the API has no read of it: GuideME's own lang files carry `guideme.lang_code`, so en_us, es_es and de_de are
  recognised; other languages need a lang file with that key); English where a page has none. UI strings in `assets/guideme/lang` (en, es, de).
- **Scenes**: `<GameScene>` with `<ImportStructure src="x.snbt"/>` (structure SNBT, read here), our tools'
  `x.json` (`{"blocks": [[x,y,z,"id[props]"] | [x,y,z,"id",{props}]]}`; a `.snbt` src falls back to the `.json` of
  the same name), inline `<Block id x y z/>`, and `<IsometricCamera yaw pitch/>`.

Markdown and tags: headings, paragraphs, **bold**, *italic*, `code`, links to pages (relative, `../x.md`),
`<ItemLink id/>`, lists (nested, numbered), quotes, code blocks, tables, rules, `<ItemImage>`, `<BlockImage>`,
`<ItemIcon>`, `<Row>`/`<Column>`/`<ItemGrid>`, `<RecipeFor id/>`, `<Recipe id/>`, `<SubPages/>`,
`<CategoryIndex category/>`, pictures (`![alt](src)` and `<img src alt/>`). Ids without a namespace are the guide's own.
- **Pictures**: `src` is read as GuideME reads it (relative to the page, `/x.png`
  from the guide's root, `ns:x.png` from that namespace's guide root). Sunrise's GUI atlas reads only `textures/`,
  so the PNG must also be at `assets/<ns>/textures/gui/ae2guide/<path>` (or `textures/gui/guide/<path>`, where our
  upstream tools put them); it is drawn at its own size, shrunk to the page's width. Without one, the alt text shows
  as `[alt]`, as before.

## Using it

- **W** over an item in any container screen (a key mapping, `key.guideme.open`, rebindable in Controls) opens its
  page; W or Esc closes it, Backspace goes back. JEI for Sunrise opens it for W over its panel and its window.
- `/guideme [namespace]` opens a guide's start page on the guide's own screen, over the world.
- Navigation tree on the left (icons, indented by parent, scrolls), Back and Contents in the title bar, the page
  scrolls with the wheel; links and item icons open their pages; item tooltips are the game's.
- The page and the tree are clipped to their panels (`DrawContext::push_clip`, GuideME's scissor): a line, a
  picture, a recipe or a scene scrolled half out shows its half, and only the shown part takes clicks.
- **Scenes** are drawn by Sunrise's `DrawContext::draw_scene` (real block models): **drag to turn** (yaw and pitch),
  **scroll over one to zoom**.
- Item tooltips get "Press W for the guide" on items with a page.

## Rust API (optional): `guideme::api`

With the dependency above, inside `mod_api::if_installed!(host, guideme, { ... })`:
`has_page(item_id) -> bool`, `open_item(item_id) -> bool` (over the open screen), `open_page(namespace,
Some("page.md") | None) -> bool` (its own screen, next frame), `is_open() -> bool`.

## Tested in game

W over the crafting table in the inventory opens its page; headings, bold/italic/code, links, item links, lists,
tables, `<RecipeFor>`, `<ItemImage>`/`<BlockImage>` rows; the navigation tree (click Furnace); scroll; the house
scene from `.snbt` (drawn with real models) and a drag that turned it; `/guideme jeidemo` on its own screen; Spanish
(title, Volver/Índice, the `_es_es` page). Unit tests (`src/tests.rs`): front matter, links, Markdown, SNBT.

## Workarounds kept, and why (mod API gaps)

- **An overlay over container screens**: `mod_screen_open` refuses while another screen is open, so W draws the
  guide in `screen_render_post` over that screen and takes its input in the `screen_*_pre` hooks. Over the world
  (`/guideme`) it is a real mod screen.
- **Entities and particles in scenes**: Sunrise's `Scene` can hold entities (`SceneEntity`) and particles
  (`SceneParticle`), but this port doesn't parse GuideME's `<Entity>` tag, and no guide page here uses one; the
  annotations below are marks, not particles.
- **Scene annotations** (`<BoxAnnotation>`, `<LineAnnotation>`, `<DiamondAnnotation>`) are not drawn: the scene's
  projection is the game's (`SceneView`), and the API doesn't hand back the matrix to place marks with.
- **Pictures** draw from the GUI atlas (`gui_sprite` + `DrawContext::sprite`), so the PNG needs a copy under
  `textures/gui/`: a PNG decoder for `ae2guide/` files would be a non-API crate (`mod_image` takes raw RGBA), and
  runtime sprites cap at 256x256 and 128 in all.
- **Block entities in scenes** draw only for mod block entities the scene names (`SceneBlockEntity`); structure
  SNBT block-entity data is not read.
