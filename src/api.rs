//! GuideME's API for other Sunrise mods, in Rust. Pages need no code (`assets/<ns>/ae2guide/`); these are for a mod that
//! wants to open the guide itself (a button, a key of its own, JEI's W over its panel). Name `guideme` under
//! `[dependencies]` in `sunrise-mod.toml` (optional is fine) and call inside
//! `mod_api::if_installed!(host, guideme, { ... })`.

use crate::{book, view};

/// Whether some guide has a page for this item (`ae2:controller`).
pub fn has_page(item: &str) -> bool {
    book::page_of_item(item).is_some()
}

/// Opens the item's page over the open screen; false when no guide has one. Call it from a client hook while a
/// screen is open (over the world, use `open_page`, which opens the guide's own screen).
pub fn open_item(item: &str) -> bool {
    let Some((g, page)) = book::page_of_item(item) else { return false };
    view::with(|v| {
        v.lang.clear();
        v.go(g, page);
    });
    true
}

/// Opens a guide's page (`index.md` when `page` is None) on the guide's own screen, on the next frame.
pub fn open_page(namespace: &str, page: Option<&str>) -> bool {
    let found = book::with_guides(|gs| {
        gs.iter().enumerate().find(|(_, g)| g.ns == namespace && !g.commands).map(|(i, g)| (i, page.map(str::to_owned).unwrap_or_else(|| g.start.clone())))
    });
    let Some(t) = found else { return false };
    view::with(|v| v.pending = Some(t));
    true
}

/// Whether the guide is open now (a mod drawing over screens can stay out of its way).
pub fn is_open() -> bool {
    view::open_or_busy()
}

/// Opens `namespace`'s commands menu (its `assets/<ns>/commands/` pages) on the guide's own screen, next frame, showing
/// only the commands of operator level `level` or less. `level` is the one the server gave the player: a mod's
/// `/<ns>` command reads it from its command source on the server and sends it, so a client never picks its own.
/// False when the mod ships no commands menu.
pub fn open_commands(namespace: &str, level: u8) -> bool {
    let Some(book) = book::commands_for(namespace, level) else { return false };
    let start = book.start.clone();
    let index = {
        let mut gs = book::GUIDES.lock().unwrap_or_else(|e| e.into_inner());
        // One commands menu at a time in the list the view reads; the guides keep their places.
        if let Some(i) = gs.iter().position(|g| g.commands) {
            gs[i] = book;
            i
        } else {
            gs.push(book);
            gs.len() - 1
        }
    };
    view::with(|v| v.pending = Some((index, start)));
    true
}

/// The level a command source runs at as a number (0: everyone, 4: owners), for [`open_commands`].
pub fn level_number(level: mod_api::PermissionLevel) -> u8 {
    use mod_api::PermissionLevel as L;
    match level {
        L::All => 0,
        L::Moderators => 1,
        L::Gamemasters => 2,
        L::Admins => 3,
        _ => 4,
    }
}
