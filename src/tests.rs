//! Unit tests of the parts that need no game: front matter, Markdown, SNBT, links.
use crate::{book, md, snbt};

#[test]
fn front_matter_and_items() {
    let p = book::parse_page("---\nnavigation:\n  title: Drive\n  parent: index.md\n  position: 20\n  icon: drive\nitem_ids:\n- ae2:drive\ncategories: [devices, storage]\n---\n\n# ME Drive\n");
    assert_eq!(p.nav.title, "Drive");
    assert_eq!(p.nav.parent.as_deref(), Some("index.md"));
    assert_eq!(p.nav.position, 20);
    assert_eq!(p.items, vec!["ae2:drive"]);
    assert_eq!(p.categories, vec!["devices", "storage"]);
    assert!(p.body.starts_with("# ME Drive"));
}

#[test]
fn links_resolve_relative() {
    assert_eq!(book::resolve("items/drive.md", "../index.md"), "index.md");
    assert_eq!(book::resolve("items/drive.md", "cell.md#top"), "items/cell.md");
    assert_eq!(book::resolve("a/b.md", "/c.md"), "c.md");
}

#[test]
fn markdown_blocks() {
    let b = md::blocks("# T\n\nSome **bold** and [a link](x.md) and <ItemLink id=\"drive\" />.\n\n- one\n- two\n\n<RecipeFor id=\"drive\" />\n\n<GameScene zoom=\"3\">\n  <ImportStructure src=\"s.snbt\" />\n  <IsometricCamera yaw=\"90\" pitch=\"20\" />\n</GameScene>\n\n| a | b |\n|---|---|\n| 1 | 2 |\n", "ae2");
    assert!(matches!(b[0], md::Block::Heading(1, _)));
    let md::Block::Para(spans) = &b[1] else { panic!("{:?}", b[1]) };
    assert!(spans.iter().any(|s| s.bold && s.text == "bold"));
    assert!(spans.iter().any(|s| s.link == Some(md::Link::Page("x.md".into()))));
    assert!(spans.iter().any(|s| s.link == Some(md::Link::Item("ae2:drive".into()))));
    assert!(matches!(b[2], md::Block::Item(0, None, _)));
    assert!(matches!(&b[4], md::Block::RecipeFor(id) if id == "ae2:drive"));
    let md::Block::Scene(s) = &b[5] else { panic!("{:?}", b[5]) };
    assert_eq!(s.structures, vec!["s.snbt"]);
    assert_eq!((s.yaw, s.pitch), (90.0, 20.0));
    let md::Block::Table(rows) = &b[6] else { panic!("{:?}", b[6]) };
    assert_eq!(rows.len(), 2);
}

#[test]
fn snbt_structure() {
    let s = "{DataVersion:4325,size:[2,1,1],palette:[{Name:\"minecraft:stone\"},{Name:\"minecraft:oak_log\",Properties:{axis:\"y\"}},{Name:\"minecraft:air\"}],blocks:[{pos:[0,0,0],state:0},{pos:[1,0,0],state:1},{pos:[1,1,0],state:2}]}";
    let b = snbt::structure(s).unwrap();
    assert_eq!(b, vec![([0, 0, 0], "minecraft:stone".to_owned()), ([1, 0, 0], "minecraft:oak_log[axis=y]".to_owned())]);
}

#[test]
fn images_keep_their_src() {
    let b = md::blocks("![The melter](tconstruct:book/meltergui.png \"t\")\n\n<img src=\"pics/a.png\" alt=\"A\" />\n", "tconstruct");
    let imgs: Vec<(String, String)> = b.iter().filter_map(|x| if let md::Block::Image(a, s) = x { Some((a.clone(), s.clone())) } else { None }).collect();
    assert_eq!(imgs, vec![("The melter".to_owned(), "tconstruct:book/meltergui.png".to_owned()), ("A".to_owned(), "pics/a.png".to_owned())]);
}

#[test]
fn click_places_cut_to_the_clip() {
    use crate::view::cut;
    let page = [100.0, 20.0, 200.0, 100.0];
    // A line half scrolled off the top keeps its lower half.
    assert_eq!(cut([110.0, 15.0, 50.0, 10.0], page), Some([110.0, 20.0, 50.0, 5.0]));
    // Wholly above or below the page: gone.
    assert_eq!(cut([110.0, 0.0, 50.0, 10.0], page), None);
    assert_eq!(cut([110.0, 120.0, 50.0, 10.0], page), None);
    // Inside: unchanged.
    assert_eq!(cut([110.0, 30.0, 50.0, 10.0], page), Some([110.0, 30.0, 50.0, 10.0]));
}

#[test]
fn table_columns_follow_their_content() {
    use crate::view::column_widths;
    let sum = |w: &[f32]| w.iter().sum::<f32>();
    // Room to spare: each column keeps its width, the rest shared evenly.
    let w = column_widths(&[100.0, 20.0, 30.0], 210.0);
    assert!((w[0] - 120.0).abs() < 0.01 && (w[1] - 40.0).abs() < 0.01 && (w[2] - 50.0).abs() < 0.01);
    // Too wide: cut in proportion to the page, the long first column still widest.
    let w = column_widths(&[300.0, 20.0, 30.0, 30.0], 200.0);
    assert!((sum(&w) - 200.0).abs() < 0.01);
    assert!(w[0] > w[1] && w[0] > w[2]);
    // A short column keeps at least a third of an even share.
    assert!(w[1] >= 200.0 / 4.0 / 3.0 * 0.99);
}

/// A commands menu keeps only the pages of the player's level or less, in every language; a page whose parent went
/// moves to the top, and the start page falls back to one still there.
#[test]
fn commands_menu_is_cut_to_the_level() {
    let page = |level: u8, parent: Option<&str>| {
        let mut p = book::parse_page(&format!("---\nlevel: {level}\n---\n# x\n"));
        p.nav.parent = parent.map(str::to_owned);
        p
    };
    let mut g = book::Guide { ns: "testmod".into(), start: "index.md".into(), commands: true, ..Default::default() };
    g.pages.insert("index.md".into(), page(0, None));
    g.pages.insert("op.md".into(), page(4, Some("index.md")));
    g.pages.insert("gm.md".into(), page(2, Some("index.md")));
    g.pages.insert("under_op.md".into(), page(0, Some("op.md")));
    g.translations.entry("es_es".into()).or_default().insert("op.md".into(), page(4, Some("index.md")));
    assert_eq!(book::parse_page("---\nlevel: 9\n---\n").level, 4);
    *book::COMMAND_BOOKS.lock().unwrap() = vec![g];
    let everyone = book::commands_for("testmod", 0).unwrap();
    assert_eq!(everyone.pages.keys().cloned().collect::<Vec<_>>(), vec!["index.md", "under_op.md"]);
    assert_eq!(everyone.pages["under_op.md"].nav.parent, None);
    assert!(everyone.translations["es_es"].is_empty());
    let gm = book::commands_for("testmod", 2).unwrap();
    assert!(gm.pages.contains_key("gm.md") && !gm.pages.contains_key("op.md"));
    let owner = book::commands_for("testmod", 4).unwrap();
    assert_eq!(owner.pages.len(), 4);
    assert_eq!(owner.pages["under_op.md"].nav.parent.as_deref(), Some("op.md"));
    assert!(book::commands_for("other", 4).is_none());
}
