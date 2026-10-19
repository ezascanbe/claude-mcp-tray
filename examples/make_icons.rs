//! Regenerates everything in `icons/` from `assets/mcp.svg`.
//! Run: cargo run --example make_icons

use resvg::{tiny_skia, usvg};
use std::fs;
use std::io::Cursor;
use std::path::Path;

const TILE: &str = "#D97757"; // Claude accent

/// The drawing inside the source SVG's root element (it uses a 200x200 viewBox).
fn glyph(src: &str) -> &str {
    let start = src.find("<svg").and_then(|i| src[i..].find('>').map(|j| i + j + 1)).expect("svg root");
    let end = src.rfind("</svg>").expect("svg end");
    &src[start..end]
}

fn tray_svg(g: &str, color: &str) -> String {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200"><g fill="{color}">{g}</g></svg>"#)
}

fn app_svg(g: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200"><rect width="200" height="200" rx="44" fill="{TILE}"/><g fill="#fff" transform="translate(36 36) scale(0.64)">{g}</g></svg>"##
    )
}

fn render(svg: &str, size: u32) -> Vec<u8> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("parse svg");
    let mut pixmap = tiny_skia::Pixmap::new(size, size).unwrap();
    let s = size as f32 / tree.size().width();
    resvg::render(&tree, tiny_skia::Transform::from_scale(s, s), &mut pixmap.as_mut());
    pixmap.encode_png().unwrap()
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = fs::read_to_string(root.join("assets/mcp.svg")).expect("assets/mcp.svg");
    let g = glyph(&src);
    let out = root.join("icons");
    fs::create_dir_all(&out).unwrap();

    // Tray: white glyph for dark taskbars, black for light ones / macOS template.
    fs::write(out.join("tray-white.png"), render(&tray_svg(g, "#ffffff"), 64)).unwrap();
    fs::write(out.join("tray-black.png"), render(&tray_svg(g, "#000000"), 64)).unwrap();
    fs::write(out.join("tray-template.png"), render(&tray_svg(g, "#000000"), 44)).unwrap();

    let app = app_svg(g);
    for (name, size) in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)] {
        fs::write(out.join(name), render(&app, size)).unwrap();
    }

    let mut ico = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16, 24, 32, 48, 64, 128, 256] {
        let img = ico::IconImage::read_png(Cursor::new(render(&app, size))).unwrap();
        ico.add_entry(ico::IconDirEntry::encode(&img).unwrap());
    }
    ico.write(fs::File::create(out.join("icon.ico")).unwrap()).unwrap();

    let mut icns = icns::IconFamily::new();
    for size in [16, 32, 64, 128, 256, 512, 1024] {
        let img = icns::Image::read_png(Cursor::new(render(&app, size))).unwrap();
        icns.add_icon(&img).unwrap();
    }
    icns.write(fs::File::create(out.join("icon.icns")).unwrap()).unwrap();

    println!("icons written to {}", out.display());
}
