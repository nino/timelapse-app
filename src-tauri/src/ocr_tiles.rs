//! Reading a big frame in tiles.
//!
//! Vision judges text size against the whole image it is given, so on a big
//! screen's frame it misses small text it reads fine in a smaller piece. On
//! test pages of a 2560×1440-point screen, reading the frame whole found about
//! a quarter of the words and reading it in tiles about four fifths. An
//! ordinary 1800×1124 frame is read whole: tiles found only a few more words
//! there and took 2.6 times as long.
//!
//! Each tile is read with some overlap into its neighbours. A line that runs
//! across a tile edge comes back from each tile cut off at that tile's edge,
//! so the pieces are trimmed to the words in their own tile and joined, and a
//! line two tiles both saw whole is kept once (`assemble`).

// Only the Vision recognizer, on macOS, reads in tiles.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::ocr::OcrLine;

/// The biggest frame read whole: the ordinary frame size.
const WHOLE_WIDTH: u32 = 1800;
const WHOLE_HEIGHT: u32 = 1124;

/// Roughly the size, in frame pixels, of each tile a bigger frame is read in:
/// 3×3 for a 2560×1440-point screen at 1.4 px per point. Smaller tiles found
/// no more text.
const TILE_WIDTH: f64 = 1200.0;
const TILE_HEIGHT: f64 = 750.0;

/// How far each tile reaches past its own cell on every side, as a share of
/// the frame, so a word on a cell boundary is whole in one of the tiles.
const TILE_OVERLAP: f64 = 0.03;

/// A line this close to a tile's inner edge (as a share of the frame) is
/// taken to run on past it.
const CUT_EDGE: f64 = 0.005;

/// One cell of the grid a frame is read in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    column: usize,
    row: usize,
    columns: usize,
    rows: usize,
}

/// A rectangle in Vision's normalized coordinates, origin bottom-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The tiles to read a `width`×`height` frame in, row by row: one for an
/// ordinary frame.
pub fn tiles(width: u32, height: u32) -> Vec<Tile> {
    let (columns, rows) = if width <= WHOLE_WIDTH && height <= WHOLE_HEIGHT {
        (1, 1)
    } else {
        (
            (width as f64 / TILE_WIDTH).ceil().max(1.0) as usize,
            (height as f64 / TILE_HEIGHT).ceil().max(1.0) as usize,
        )
    };
    (0..rows)
        .flat_map(|row| (0..columns).map(move |column| Tile { column, row, columns, rows }))
        .collect()
}

impl Tile {
    fn cell_left(&self) -> f64 {
        self.column as f64 / self.columns as f64
    }

    fn cell_right(&self) -> f64 {
        (self.column + 1) as f64 / self.columns as f64
    }

    /// The part of the frame Vision reads for this tile: its cell grown by
    /// `TILE_OVERLAP`.
    pub fn region(&self) -> Region {
        let left = (self.cell_left() - TILE_OVERLAP).max(0.0);
        let right = (self.cell_right() + TILE_OVERLAP).min(1.0);
        let top = (self.row as f64 / self.rows as f64 - TILE_OVERLAP).max(0.0);
        let bottom = ((self.row + 1) as f64 / self.rows as f64 + TILE_OVERLAP).min(1.0);
        Region { x: left, y: 1.0 - bottom, width: right - left, height: bottom - top }
    }
}

/// A line one tile read, in whole-frame coordinates, and which of the tile's
/// inner edges it runs into.
#[derive(Debug, Clone)]
struct Piece {
    line: OcrLine,
    tile: Tile,
    cut_left: bool,
    cut_right: bool,
    /// Into the top or bottom edge: a line on a row boundary, which the tile
    /// above or below sees whole.
    cut_across: bool,
}

impl Piece {
    fn new(tile: Tile, line: OcrLine) -> Piece {
        let region = tile.region();
        let line = OcrLine {
            x: region.x + line.x * region.width,
            y: region.y + line.y * region.height,
            width: line.width * region.width,
            height: line.height * region.height,
            ..line
        };
        let right = line.x + line.width;
        let region_right = region.x + region.width;
        let region_top = region.y + region.height;
        Piece {
            cut_left: region.x > 0.0 && line.x - region.x < CUT_EDGE,
            cut_right: region_right < 1.0 && region_right - right < CUT_EDGE,
            cut_across: (region.y > 0.0 && line.y - region.y < CUT_EDGE)
                || (region_top < 1.0 && region_top - (line.y + line.height) < CUT_EDGE),
            line,
            tile,
        }
    }

    fn is_whole(&self) -> bool {
        !(self.cut_left || self.cut_right || self.cut_across)
    }
}

fn right(line: &OcrLine) -> f64 {
    line.x + line.width
}

fn top(line: &OcrLine) -> f64 {
    line.y + line.height
}

/// Whether two lines share most of their height: the same row of text.
fn same_row(a: &OcrLine, b: &OcrLine) -> bool {
    let overlap = top(a).min(top(b)) - a.y.max(b.y);
    overlap >= 0.5 * a.height.min(b.height)
}

/// Whether `outer` spans `inner` across, in the same row.
fn spans(outer: &OcrLine, inner: &OcrLine) -> bool {
    same_row(outer, inner) && outer.x <= inner.x + 0.01 && right(outer) >= right(inner) - 0.01
}

/// `piece`'s line keeping only the words whose centre is in its tile's
/// column, on the sides where it is cut, and never the word at a cut edge,
/// which is usually part of a word (the neighbouring tile sees it whole). The
/// words' places are estimated from their characters' places in the text, as
/// if every character were as wide.
fn trim(piece: &Piece) -> Option<OcrLine> {
    let line = &piece.line;
    let chars: Vec<char> = line.text.chars().collect();
    let total = chars.len().max(1) as f64;
    let at = |index: usize| line.x + line.width * index as f64 / total;
    let mut words = Vec::new();
    let mut start = None;
    for (i, c) in chars.iter().chain(std::iter::once(&' ')).enumerate() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                words.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if piece.cut_left && !words.is_empty() {
        words.remove(0);
    }
    if piece.cut_right {
        words.pop();
    }
    let kept: Vec<(usize, usize)> = words
        .into_iter()
        .filter(|&(s, e)| {
            let centre = (at(s) + at(e)) / 2.0;
            !(piece.cut_left && centre < piece.tile.cell_left())
                && !(piece.cut_right && centre >= piece.tile.cell_right())
        })
        .collect();
    let (&(first, _), &(_, last)) = (kept.first()?, kept.last()?);
    let text = kept.iter().map(|&(s, e)| chars[s..e].iter().collect::<String>()).collect::<Vec<_>>().join(" ");
    Some(OcrLine { text, x: at(first), width: at(last) - at(first), ..line.clone() })
}

/// Join `right_line` onto the end of `left_line`.
fn join(left_line: OcrLine, right_line: OcrLine) -> OcrLine {
    let y = left_line.y.min(right_line.y);
    OcrLine {
        text: format!("{} {}", left_line.text, right_line.text),
        confidence: left_line.confidence.min(right_line.confidence),
        x: left_line.x,
        y,
        width: right(&right_line) - left_line.x,
        height: top(&left_line).max(top(&right_line)) - y,
    }
}

/// Put together the lines each tile read (boxes relative to the tile's
/// region, as Vision reports them) into the frame's lines, top to bottom and
/// left to right.
pub fn assemble(read: Vec<(Tile, Vec<OcrLine>)>) -> Vec<OcrLine> {
    if let [(tile, lines)] = &read[..] {
        if tile.columns == 1 && tile.rows == 1 {
            // Read whole: the lines as Vision gave them.
            return lines.clone();
        }
    }
    let pieces: Vec<Piece> = read
        .into_iter()
        .flat_map(|(tile, lines)| lines.into_iter().map(move |line| Piece::new(tile, line)))
        .collect();
    let (whole, cut): (Vec<Piece>, Vec<Piece>) = pieces.into_iter().partition(Piece::is_whole);

    // A line in the overlap is seen whole by both tiles: keep one.
    let mut lines: Vec<OcrLine> = Vec::new();
    for piece in whole {
        let duplicate = lines.iter_mut().find(|kept| {
            let overlap = right(kept).min(right(&piece.line)) - kept.x.max(piece.line.x);
            same_row(kept, &piece.line) && overlap >= 0.5 * kept.width.min(piece.line.width)
        });
        match duplicate {
            Some(kept) if piece.line.confidence > kept.confidence => *kept = piece.line,
            Some(_) => {}
            None => lines.push(piece.line),
        }
    }

    // A cut piece another tile saw whole adds nothing. The rest are trimmed
    // to their own words and joined left to right.
    let mut fragments: Vec<(Piece, OcrLine)> = cut
        .into_iter()
        .filter(|piece| !lines.iter().any(|line| spans(line, &piece.line)))
        .filter(|piece| !piece.cut_across || !(piece.cut_left || piece.cut_right))
        .filter_map(|piece| trim(&piece).map(|line| (piece, line)))
        .collect();
    fragments.sort_by(|(a, _), (b, _)| a.tile.column.cmp(&b.tile.column).then(a.line.x.total_cmp(&b.line.x)));
    let mut used = vec![false; fragments.len()];
    for i in 0..fragments.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let (mut piece, mut line) = fragments[i].clone();
        while piece.cut_right {
            let next = (0..fragments.len()).find(|&j| {
                let (other, other_line) = &fragments[j];
                !used[j]
                    && other.cut_left
                    && other.tile.column == piece.tile.column + 1
                    && other.tile.row == piece.tile.row
                    && same_row(&piece.line, &other.line)
                    // The trimmed pieces stop short of the boundary by a
                    // word each, and the widths are estimates.
                    && (other_line.x - right(&line)).abs() < 5.0 * TILE_OVERLAP
            });
            let Some(j) = next else { break };
            used[j] = true;
            line = join(line, fragments[j].1.clone());
            piece = fragments[j].0.clone();
        }
        lines.push(line);
    }

    in_reading_order(lines)
}

/// Rows top to bottom (a line joins the row above when it shares most of its
/// height), each left to right.
fn in_reading_order(mut lines: Vec<OcrLine>) -> Vec<OcrLine> {
    lines.sort_by(|a, b| top(b).total_cmp(&top(a)));
    let mut rows: Vec<Vec<OcrLine>> = Vec::new();
    for line in lines {
        match rows.last_mut() {
            Some(row) if same_row(&row[0], &line) => row.push(line),
            _ => rows.push(vec![line]),
        }
    }
    rows.into_iter()
        .flat_map(|mut row| {
            row.sort_by(|a, b| a.x.total_cmp(&b.x));
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line at frame coordinates, as `tile` would report it.
    fn seen_by(tile: Tile, text: &str, x: f64, y: f64, width: f64, height: f64) -> OcrLine {
        let region = tile.region();
        OcrLine {
            text: text.into(),
            confidence: 1.0,
            x: (x - region.x) / region.width,
            y: (y - region.y) / region.height,
            width: width / region.width,
            height: height / region.height,
        }
    }

    fn texts(lines: &[OcrLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn ordinary_frames_are_read_whole_and_bigger_ones_in_tiles() {
        assert_eq!(tiles(1800, 1124).len(), 1);
        assert_eq!(tiles(1800, 1124)[0].region(), Region { x: 0.0, y: 0.0, width: 1.0, height: 1.0 });
        // A 2560×1440-point screen at 2x (1.4 px a point), and at 1x.
        assert_eq!(tiles(3584, 2016).len(), 9);
        assert_eq!(tiles(2560, 1440).len(), 6);
        assert_eq!(tiles(3584, 2016)[1], Tile { column: 1, row: 0, columns: 3, rows: 3 });
    }

    #[test]
    fn a_tile_reads_its_cell_and_a_margin() {
        let [top_left, .., bottom_right] = tiles(2400, 1500)[..] else { panic!() };
        let region = top_left.region();
        assert_eq!((region.x, region.width), (0.0, 0.53));
        assert!((region.y - 0.47).abs() < 1e-9);
        let region = bottom_right.region();
        assert!((region.x - 0.47).abs() < 1e-9);
        assert_eq!(region.y, 0.0);
    }

    #[test]
    fn boxes_move_into_frame_coordinates() {
        let [_, _, _, bottom_right] = tiles(2400, 1500)[..] else { panic!() };
        let lines = assemble(vec![(bottom_right, vec![seen_by(bottom_right, "hello", 0.6, 0.2, 0.1, 0.02)])]);
        let line = &lines[0];
        assert!((line.x - 0.6).abs() < 1e-9 && (line.y - 0.2).abs() < 1e-9);
        assert!((line.width - 0.1).abs() < 1e-9 && (line.height - 0.02).abs() < 1e-9);
    }

    #[test]
    fn a_line_cut_by_a_tile_edge_is_joined_without_repeats() {
        let [left, right_tile, ..] = tiles(2400, 1500)[..] else { panic!() };
        // "the quick encoder finishes another batch" runs from 0.2 to 0.8; the
        // left tile reads it up to its edge at 0.53, the right one from 0.47,
        // each cutting a word.
        let left_piece = seen_by(left, "the quick encoder fini", 0.2, 0.7, 0.33, 0.02);
        let right_piece = seen_by(right_tile, "der finishes another batch", 0.47, 0.7, 0.33, 0.02);

        let lines = assemble(vec![(left, vec![left_piece]), (right_tile, vec![right_piece])]);

        assert_eq!(texts(&lines), ["the quick encoder finishes another batch"]);
        assert!((lines[0].x - 0.2).abs() < 1e-9);
        assert!((right(&lines[0]) - 0.8).abs() < 1e-9);
    }

    #[test]
    fn a_line_both_tiles_see_whole_is_kept_once() {
        let [left, right_tile, ..] = tiles(2400, 1500)[..] else { panic!() };
        // "Settings", centred on the boundary, inside both regions; the two
        // tiles place it slightly differently.
        let lines = assemble(vec![
            (left, vec![seen_by(left, "Settings", 0.48, 0.9, 0.04, 0.02)]),
            (right_tile, vec![seen_by(right_tile, "Settings", 0.485, 0.9, 0.04, 0.02)]),
        ]);
        assert_eq!(texts(&lines), ["Settings"]);
    }

    #[test]
    fn a_piece_of_a_line_another_tile_saw_whole_is_dropped() {
        let [left, right_tile, ..] = tiles(2400, 1500)[..] else { panic!() };
        // Whole in the left tile (0.3–0.52), cut at the right tile's left edge.
        let lines = assemble(vec![
            (left, vec![seen_by(left, "open the settings", 0.3, 0.5, 0.22, 0.02)]),
            (right_tile, vec![seen_by(right_tile, "tings", 0.47, 0.5, 0.05, 0.02)]),
        ]);
        assert_eq!(texts(&lines), ["open the settings"]);
    }

    #[test]
    fn lines_come_out_in_reading_order() {
        let [left, right_tile, ..] = tiles(2400, 1500)[..] else { panic!() };
        // Two lines in one row, the right one a hair higher, and one below.
        let lines = assemble(vec![
            (left, vec![seen_by(left, "first", 0.1, 0.8, 0.1, 0.02), seen_by(left, "third", 0.1, 0.7, 0.1, 0.02)]),
            (right_tile, vec![seen_by(right_tile, "second", 0.7, 0.801, 0.1, 0.02)]),
        ]);
        assert_eq!(texts(&lines), ["first", "second", "third"]);
    }
}
