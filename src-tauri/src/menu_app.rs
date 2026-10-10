//! Which app was in front, read from the OCR text of the macOS menu bar.
//!
//! Screenshots taken before the capture loop recorded the front window
//! (`screenshots.window_id`) have only their OCR text to go on. The menu bar
//! names the front app in its first item, right of the Apple logo, followed
//! by the app's menus (`Blender  File  Edit  Render  Window  Help`). Vision
//! reads those items as separate lines or merges a few into one
//! (`Claude File Edit`), so the name is the leftmost menu-bar line up to the
//! first standard menu title.
//!
//! This is a guess: a frame without a menu bar (the lock screen, a
//! full-screen app) or one where Vision read the bar differently gets `None`.

use crate::database::LineBox;

/// Menu titles that follow the app's own menu. The name is cut at the first
/// of these, so a line that merged it with the menus still gives the name,
/// and a menu bar must show one of them to count as a menu bar.
const MENU_TITLES: &[&str] = &[
    "file", "edit", "view", "window", "help", "go", "shell", "format", "insert", "history",
    "bookmarks", "navigate", "selection", "tab", "chat", "call", "create",
];

/// How far below the highest line's top edge a line's top may be and still
/// be in the menu bar, as a fraction of the frame's height. The menu bar's
/// items sit within a few thousandths of each other; the next row of text
/// (a window's title bar or tabs) is about 0.035 lower.
const MENU_BAR_BAND: f64 = 0.015;

/// How far from the left edge the app's name may start, as a fraction of the
/// frame's width. It starts at about 0.05, or 0.02 on a frame letterboxed
/// from a wider screen; a line further right is not the first menu item.
const MAX_NAME_LEFT: f64 = 0.12;

/// The front app's name, read from the menu bar in `text` (one OCR line per
/// row) and `boxes` (one per line, as stored in `ocr_frames`).
pub fn menu_bar_app(text: &str, boxes: &[LineBox]) -> Option<String> {
    let lines: Vec<(&str, &LineBox)> = text.lines().zip(boxes).collect();
    let top = |b: &LineBox| b[1] + b[3];
    let highest = lines.iter().map(|(_, b)| top(b)).fold(f64::NEG_INFINITY, f64::max);
    let mut bar: Vec<&(&str, &LineBox)> = lines
        .iter()
        .filter(|(_, b)| top(b) >= highest - MENU_BAR_BAND)
        .collect();
    bar.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));

    let is_menu_title = |word: &str| MENU_TITLES.contains(&word.to_lowercase().as_str());
    let has_menus = bar
        .iter()
        .any(|(line, _)| line.split_whitespace().any(is_menu_title));
    let (first, first_box) = bar.first()?;
    if !has_menus || first_box[0] > MAX_NAME_LEFT {
        return None;
    }

    // The Apple logo, when Vision reads it at all, comes out as a stray
    // symbol in front of the name.
    let name: Vec<&str> = first
        .split_whitespace()
        .skip_while(|word| !word.chars().any(char::is_alphanumeric))
        .take_while(|word| !is_menu_title(word))
        .collect();
    (!name.is_empty()).then(|| name.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OCR lines as (text, x, y, width, height), as Vision reports them.
    fn read(lines: &[(&str, f64, f64, f64, f64)]) -> Option<String> {
        let text = lines.iter().map(|l| l.0).collect::<Vec<_>>().join("\n");
        let boxes: Vec<LineBox> = lines.iter().map(|l| [l.1, l.2, l.3, l.4]).collect();
        menu_bar_app(&text, &boxes)
    }

    // The cases below are menu bars from Nino's library, as OCR read them.

    #[test]
    fn reads_a_name_vision_read_on_its_own() {
        let frame = [
            ("Obsidian", 0.0523, 0.9766, 0.0393, 0.0143),
            ("mbn2024.local", 0.0828, 0.9442, 0.0640, 0.0116),
            ("File", 0.1017, 0.9767, 0.0145, 0.0139),
            ("Edit", 0.1279, 0.9767, 0.0174, 0.0139),
            ("Telegram", 0.7907, 0.9209, 0.0378, 0.0139),
            ("Q Sep 25 19:14 o", 0.8836, 0.9740, 0.0933, 0.0187),
        ];
        assert_eq!(read(&frame).as_deref(), Some("Obsidian"));
    }

    #[test]
    fn cuts_a_name_merged_with_the_menus() {
        let frame = [
            ("Claude File Edit", 0.0538, 0.9767, 0.0843, 0.0144),
            ("View", 0.1439, 0.9767, 0.0262, 0.0139),
            ("Go Window Help", 0.1701, 0.9767, 0.0959, 0.0144),
        ];
        assert_eq!(read(&frame).as_deref(), Some("Claude"));
        let frame = [
            ("Timelapse App", 0.0523, 0.9742, 0.0640, 0.0142),
            ("Cost Tracker - Google She", 0.0930, 0.9440, 0.0959, 0.0119),
            ("Edit View Window", 0.1453, 0.9767, 0.0988, 0.0144),
        ];
        assert_eq!(read(&frame).as_deref(), Some("Timelapse App"));
    }

    #[test]
    fn reads_a_menu_bar_letterboxed_lower_in_the_frame() {
        let frame = [
            ("Ghostty File", 0.0204, 0.9349, 0.0421, 0.0117),
            ("• Cost", 0.0596, 0.8977, 0.0218, 0.0116),
            ("Edit", 0.0611, 0.9349, 0.0145, 0.0116),
            ("Help", 0.1265, 0.9349, 0.0145, 0.0116),
            ("Co Sep 24 20:15", 0.9433, 0.9368, 0.0509, 0.0074),
        ];
        assert_eq!(read(&frame).as_deref(), Some("Ghostty"));
    }

    #[test]
    fn needs_only_one_menu_title() {
        let frame = [
            ("Blender", 0.0523, 0.9766, 0.0349, 0.0143),
            ("* Catio Marne Avenue1.blend - Blender 5.2.2 LTS", 0.0828, 0.9419, 0.2006, 0.0139),
            ("Window", 0.0974, 0.9767, 0.0334, 0.0139),
            ("Sep 27 14:32 O", 0.9084, 0.9767, 0.0683, 0.0144),
        ];
        assert_eq!(read(&frame).as_deref(), Some("Blender"));
    }

    #[test]
    fn finds_nothing_without_a_menu_bar() {
        // The lock screen.
        let frame = [
            ("22:39", 0.3866, 0.7581, 0.2253, 0.1046),
            ("Oct 4 Sun", 0.4607, 0.8788, 0.0786, 0.0238),
        ];
        assert_eq!(read(&frame), None);
        // Only the menu bar's right-hand side.
        assert_eq!(read(&[("ABC E", 0.8852, 0.9767, 0.0334, 0.0144)]), None);
        assert_eq!(read(&[]), None);
    }

    #[test]
    fn finds_nothing_when_the_name_was_not_read() {
        let frame = [
            ("File", 0.0523, 0.9767, 0.0160, 0.0139),
            ("Edit View Go Window", 0.1177, 0.9767, 0.1148, 0.0144),
        ];
        assert_eq!(read(&frame), None);
    }

    #[test]
    fn skips_a_symbol_read_from_the_apple_logo() {
        let frame = [
            ("@ Finder File", 0.0300, 0.9767, 0.0600, 0.0139),
            ("Edit", 0.1177, 0.9767, 0.0174, 0.0139),
        ];
        assert_eq!(read(&frame).as_deref(), Some("Finder"));
    }
}
