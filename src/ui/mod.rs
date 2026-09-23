pub mod analytics;
pub mod theme;
pub mod day_view;
pub mod overview;
pub mod quiz_window;
pub mod reviews;
pub mod roadmap;
pub mod settings;
pub mod study;
pub mod subjects;
pub mod weekly;

/// Every non-ASCII character the UI draws has to exist in egui's bundled
/// fonts; a missing one renders as an empty box, with nothing reported
/// anywhere. The Proportional family is `[Ubuntu-Light, NotoEmoji,
/// emoji-icon-font]` and Monospace is that same list behind Hack, so
/// Monospace strictly covers more: a glyph only Hack carries has to be asked
/// for with `.monospace()` at the call site.
#[cfg(test)]
mod glyph_coverage {
    use egui::epaint::text::{FontDefinitions, Fonts, TextOptions};
    use std::path::{Path, PathBuf};

    /// Glyphs Hack alone supplies. There is no check mark in the Proportional
    /// family at all, so every one of these is drawn `.monospace()`.
    const MONOSPACE_ONLY: &[char] = &['\u{2714}'];

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("src is readable").flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// Non-ASCII characters a line can put on screen, counting both literal
    /// characters and `\u{...}` escapes. Comment lines are prose, never drawn.
    fn rendered_chars(source: &str) -> Vec<char> {
        let mut found = Vec::new();
        for line in source.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            found.extend(line.chars().filter(|c| !c.is_ascii()));

            let mut rest = line;
            while let Some(at) = rest.find("\\u{") {
                rest = &rest[at + 3..];
                let Some(end) = rest.find('}') else { break };
                if let Ok(code) = u32::from_str_radix(&rest[..end], 16) {
                    found.extend(char::from_u32(code));
                }
                rest = &rest[end + 1..];
            }
        }
        found
    }

    #[test]
    fn every_glyph_the_ui_draws_exists_in_the_bundled_fonts() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        assert!(!files.is_empty(), "there are sources to scan");

        let mut fonts = Fonts::new(TextOptions::default(), FontDefinitions::default());
        let proportional = egui::FontId::proportional(14.0);
        let monospace = egui::FontId::monospace(14.0);

        let mut missing = Vec::new();
        for file in &files {
            let source = std::fs::read_to_string(file).expect("source is UTF-8");
            for c in rendered_chars(&source) {
                let (family, note) = if MONOSPACE_ONLY.contains(&c) {
                    (&monospace, "monospace")
                } else {
                    (&proportional, "proportional")
                };
                if !fonts.has_glyph(family, c) {
                    let name = file.file_name().unwrap_or(file.as_os_str());
                    missing.push(format!(
                        "U+{:04X} {c:?} ({note}) in {}",
                        c as u32,
                        name.to_string_lossy()
                    ));
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "these characters have no glyph and render as empty boxes:\n{}",
            missing.join("\n")
        );
    }
}
