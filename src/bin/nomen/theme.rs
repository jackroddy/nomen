//! Colors, and turning one suggestion into a styled line.

use nomen::{Availability, ExpandedSlot, Lang, Suggestion};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Colors for matched letters, by slot position.
//
// eight hues picked to stay mutually distinguishable, with no red/green pair
// adjacent -- telling the pairs apart is the entire purpose. Eight is enough
// because Config::max_len caps names at 8 letters.
const PALETTE: [Color; 8] = [
    Color::Indexed(39),  // blue
    Color::Indexed(208), // orange
    Color::Indexed(42),  // green
    Color::Indexed(201), // magenta
    Color::Indexed(226), // yellow
    Color::Indexed(51),  // cyan
    Color::Indexed(141), // purple
    Color::Indexed(203), // salmon
];

pub fn mark(status: Availability) -> Span<'static> {
    match status {
        Availability::Available => Span::styled("✓", Style::default().fg(Color::Green)),
        Availability::Taken => Span::styled("·", Style::default().fg(Color::DarkGray)),
        Availability::Unknown => Span::styled("?", Style::default().fg(Color::DarkGray)),
    }
}

/// One results row: availability, the acronym, its expansion, and — for a
/// classical entry — the original spelling and gloss.
/// Columns before the expansion starts: the availability mark, a space, and
/// the acronym padded out.
const NAME_COL: usize = 2 + NAME_WIDTH;
const NAME_WIDTH: usize = 11;
/// Least space worth starting a gloss in.
const MIN_GLOSS: usize = 22;

/// What the results box knows about a row that the row does not.
#[derive(Clone, Copy)]
pub struct RowLayout {
    /// Terminal columns the row has.
    pub width: u16,
    /// Columns the expansion needs, from [`App::expansion_width`].
    pub expansion: usize,
    /// Name the language the entry came from. Only the `all` tab needs it —
    /// on any other, the tab has already said.
    pub lang_tag: bool,
}

impl RowLayout {
    /// Column the gloss starts in. Rows whose expansion runs past it push
    /// their own gloss right rather than being cut short: the expansion is the
    /// answer, the gloss only annotates it.
    fn gloss_col(self) -> usize {
        let want = NAME_COL + self.expansion + 2;
        want.min((self.width as usize).saturating_sub(MIN_GLOSS))
    }
}

pub fn row(s: &Suggestion, status: Availability, layout: RowLayout) -> Line<'static> {
    let mut spans = vec![mark(status), Span::raw(" ")];

    for (i, c) in s.name.char_indices() {
        let up = c.to_ascii_uppercase().to_string();
        match s.expansion.get(i) {
            Some(ExpandedSlot::Gap) | None => {
                spans.push(Span::styled(up, Style::default().add_modifier(Modifier::DIM)));
            }
            _ => spans.push(Span::styled(up, Style::default().fg(PALETTE[i % PALETTE.len()]))),
        }
    }
    spans.push(Span::raw(" ".repeat(NAME_WIDTH.saturating_sub(s.name.len()))));

    for (i, slot) in s.expansion.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let color = PALETTE[i % PALETTE.len()];
        match slot {
            ExpandedSlot::Block { word, letter, .. } => raise(&mut spans, word, *letter as usize, color),
            ExpandedSlot::SelfRef { word } => raise(&mut spans, word, 0, color),
            ExpandedSlot::Gap => {
                let letter = s.name.as_bytes()[i] as char;
                spans.push(Span::styled(
                    letter.to_string(),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
        }
    }

    if s.lang != Lang::English {
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let indent = layout.gloss_col().saturating_sub(used).max(2);
        let dim = Style::default().add_modifier(Modifier::DIM);

        let tag = if layout.lang_tag { format!("{} ", s.lang.tag()) } else { String::new() };
        let room = (layout.width as usize).saturating_sub(used + indent + 1);
        // the tag, the display form and its quotes are the fixed cost of one
        let fixed = tag.chars().count() + s.display.chars().count() + 5;
        if room > fixed + 4 {
            spans.push(Span::raw(" ".repeat(indent)));
            spans.push(Span::styled(format!("[{tag}"), dim));
            // the original spelling is a foreign word set inside English, and
            // italic is what that has always meant
            spans.push(Span::styled(
                s.display.clone(),
                dim.add_modifier(Modifier::ITALIC),
            ));
            spans.push(Span::styled(
                format!(" “{}”]", truncate(&s.gloss, room - fixed)),
                dim,
            ));
        }
    }

    Line::from(spans)
}

/// Upper-case and color the letter a word contributes, so an interior match
/// reads as `coDe` and the acronym stays legible.
fn raise(spans: &mut Vec<Span<'static>>, word: &str, at: usize, color: Color) {
    for (i, c) in word.char_indices() {
        if i == at {
            spans.push(Span::styled(c.to_ascii_uppercase().to_string(), Style::default().fg(color)));
        } else {
            spans.push(Span::raw(c.to_string()));
        }
    }
}

/// Cut to `width` display columns, counting characters rather than bytes so a
/// Greek gloss is not split mid-codepoint.
fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}
