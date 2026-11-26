//! GuideME for Sunrise: AE2's guidebook (`appeng.client.guidebook`, LGPL-3.0) as a library mod that
//! every Sunrise mod shares. A mod ships its pages under `assets/<namespace>/ae2guide/` and nothing else: no code, no
//! Rust dependency (Sunrise's packages can't depend on each other's crates). This mod reads every namespace's pages
//! with a client reload listener and shows them:
//!
//! - W (a key mapping, Options > Controls) over an item in any container screen opens that item's page, over the
//!   screen (Sunrise's `mod_screen_open` refuses while another screen is open, so it is an overlay there);
//! - `/guideme [namespace]` opens a guide's start page on the mod's own screen;
//! - the item tooltip says so ("Hold W: guide") for the items with a page.
//!
//! Pages are AE2's Markdown with its tags (`md.rs`), translated by `_<lang>/` folders, with 3D scenes drawn by the
//! Sunrise's `DrawContext::draw_scene` that turn by dragging and zoom by scrolling.

pub mod api;
mod book;
mod md;
mod snbt;
mod view;

use mod_api::*;

pub(crate) const ID: &str = "guideme";

#[derive(Default)]
struct Mod;

/// The open key's handle, resolved on first use and again when the client's roster was replaced.
static OPEN_KEY: std::sync::Mutex<Option<ModKeyMappingRef>> = std::sync::Mutex::new(None);

fn open_key(host: &dyn ClientHost) -> String {
    let mut cached = OPEN_KEY.lock().unwrap_or_else(|e| e.into_inner());
    for _ in 0..2 {
        let mapping = match *cached {
            Some(m) => m,
            None => match host.mod_key_mapping(ID, "open") {
                Ok(m) => *cached.insert(m),
                Err(_) => break,
            },
        };
        match mapping.binding(host) {
            Ok((k, _)) => return k.to_owned(),
            Err(ModKeyRefusal::Stale) => *cached = None,
            Err(_) => break,
        }
    }
    "key.keyboard.w".to_owned()
}

#[mod_api::sunrise_mod]
impl Hooks for Mod {
    fn register(&self, r: &mut dyn Registrar, _cx: &RegistrationContext) {
        r.register_key_mapping(
            KeyRegistration::new("open", "key.keyboard.w").label("key.guideme.open").context(KeyConflictContext::Gui),
        );
    }

    fn add_client_reload_listeners(&self, _context: &dyn ClientReloadListenerContext, event: &mut AddClientReloadListenersEvent<'_>) {
        for (id, folder, commands) in [("guideme:pages", "ae2guide", false), ("guideme:commands", "commands", true)] {
            let (Ok(id), Ok(conv)) = (Identifier::parse(id), FileToIdConverter::new(folder, "")) else { continue };
            if let Err(e) = event.listeners.add_listener(id, conv, book::listener(commands)) {
                eprintln!("[guideme] {folder} not read: {e:?}");
            }
        }
    }

    fn register_client_commands(&self, _host: &mut dyn ClientHost, event: &mut RegisterClientCommandsEvent<'_>) {
        event.commands.push(Command::new("guideme [namespace:word] [page:text]"));
    }

    fn command(&self, _host: &mut dyn ClientHost, event: &mut CommandEvent<'_>) -> Flow {
        let mut words = event.command.trim_start_matches('/').split_whitespace();
        if words.next() != Some("guideme") {
            return Flow::Continue;
        }
        let ns = words.next();
        // `/guideme <namespace> <page.md>` opens that page (a test's way in); an unknown page, the start page.
        let page = words.next();
        let target = book::with_guides(|gs| {
            gs.iter()
                .enumerate()
                .find(|(_, g)| !g.commands && ns.is_none_or(|n| g.ns == n))
                .map(|(i, g)| (i, page.filter(|p| g.pages.contains_key(*p)).map(str::to_owned).unwrap_or_else(|| g.start.clone())))
        });
        if let Some(t) = target {
            view::with(|v| v.pending = Some(t));
        }
        Flow::Cancel
    }

    fn datapack_sync(&self, _host: &mut dyn ServerHost, event: &mut DatapackSyncEvent<'_>) {
        // The recipes the pages show, from the server's table (mods' data included), and the guides' own types.
        let extra: Vec<String> = book::with_guides(|gs| gs.iter().flat_map(|g| g.recipe_types.clone()).collect());
        for t in view::VANILLA_KINDS.iter().map(|s| (*s).to_owned()).chain(extra) {
            if !event.recipe_types.contains(&t) {
                event.recipe_types.push(t);
            }
        }
    }

    fn render_frame_pre(&self, host: &mut dyn ClientHost, _event: &mut RenderFramePreEvent) {
        let Some((g, page)) = view::with(|v| v.pending.take()) else { return };
        // The chat screen the command came from closes first; then the mod's own screen can open.
        if host.mod_screen_open(ID) {
            view::with(|v| {
                v.close();
                v.lang = view::language(host);
                v.go(g, page);
                v.own_screen = true;
            });
        } else {
            view::with(|v| v.pending = Some((g, page)));
        }
    }

    fn item_tooltip(&self, host: &dyn ClientHost, event: &mut ItemTooltipEvent<'_>) {
        if view::open_or_busy() {
            return;
        }
        let id = event.stack.item().id(host).unwrap_or_default().to_owned();
        if book::page_of_item(&id).is_some() {
            let key = open_key(host);
            let name = key.rsplit('.').next().unwrap_or("w").to_uppercase();
            let line = host
                .translate("guideme.hint")
                .map(|t| t.replacen("%s", &name, 1))
                .unwrap_or_else(|| format!("Press {name} for the guide"));
            // Dev 7cfdf81: tooltip lines are a live view; push takes the text and colour.
            let _ = event.lines.push(Text::literal(line), 0xFF80_8080);
        }
    }

    fn screen_render_post(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        let own = view::with(|v| v.own_screen);
        if own && host.mod_screen() != Some(ID) {
            return;
        }
        view::draw(host, &mut *event.draw, event.screen.mouse, host.millis());
    }

    fn screen_key_pressed_pre(&self, host: &dyn ClientHost, event: &mut ScreenKeyPressedPreEvent<'_>) -> Flow {
        if overlay_screen(event.screen.id) {
            return Flow::Continue;
        }
        let key = open_key(host);
        if view::with(|v| v.open) {
            match event.key {
                "key.keyboard.escape" => view::with(|v| v.close()),
                "key.keyboard.backspace" => view::with(|v| v.back()),
                k if k == key && !view::with(|v| v.own_screen) => view::with(|v| v.close()),
                _ => {}
            }
            // Escape on our own screen also closes the screen itself.
            return if event.key == "key.keyboard.escape" && view::with(|v| v.own_screen) { Flow::Continue } else { Flow::Cancel };
        }
        if event.key == key
            && let Some(item) = host.screen_hovered_stack().map(|s| s.item())
            && let Some((g, page)) = book::page_of_item(item.id(host).unwrap_or_default())
        {
            view::with(|v| {
                v.lang = view::language(host);
                v.go(g, page);
            });
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn screen_mouse_button_pressed_pre(&self, _host: &dyn ClientHost, event: &mut ScreenMouseButtonPressedPreEvent<'_>) -> Flow {
        if overlay_screen(event.screen.id) {
            return Flow::Continue;
        }
        if view::click(event.screen.mouse, event.button) { Flow::Cancel } else { Flow::Continue }
    }

    fn screen_mouse_button_released_pre(&self, _host: &dyn ClientHost, event: &mut ScreenMouseButtonReleasedPreEvent<'_>) -> Flow {
        if overlay_screen(event.screen.id) {
            return Flow::Continue;
        }
        if view::release() { Flow::Cancel } else { Flow::Continue }
    }

    fn screen_mouse_dragged_pre(&self, _host: &dyn ClientHost, event: &mut ScreenMouseDraggedPreEvent<'_>) -> Flow {
        if overlay_screen(event.screen.id) {
            return Flow::Continue;
        }
        if view::drag(event.delta) { Flow::Cancel } else { Flow::Continue }
    }

    fn screen_mouse_scrolled_pre(&self, _host: &dyn ClientHost, event: &mut ScreenMouseScrolledPreEvent<'_>) -> Flow {
        if overlay_screen(event.screen.id) {
            return Flow::Continue;
        }
        if view::scroll(event.screen.mouse, event.scroll[1] as f32) { Flow::Cancel } else { Flow::Continue }
    }

    fn screen_char_typed_pre(&self, _host: &dyn ClientHost, event: &mut ScreenCharTypedPreEvent<'_>) -> Flow {
        if overlay_screen(event.screen.id) {
            return Flow::Continue;
        }
        if view::with(|v| v.open) { Flow::Cancel } else { Flow::Continue }
    }

    fn screen_closing(&self, _host: &dyn ClientHost, _event: &mut ScreenClosingEvent<'_>) {
        view::with(|v| v.close());
    }
}
#[cfg(test)]
mod tests;

/// The prompts and dialogs Dev f8aa80e's screen input Pre hooks newly reach (debt-59-mounted-screen-widgets): the hooks
/// leave them alone, as on 7cfdf81, where these hooks never saw them.
fn overlay_screen(id: &str) -> bool {
    matches!(
        id,
        "command_prompt" | "link_prompt" | "code_of_conduct" | "server_pack_prompt" | "dialog" | "book_view" | "book_edit" | "book_sign" | "lectern" | "sign_editor"
    )
}
