//! The transcript's copy affordances: a hover button per entry that
//! copies it as plain text, and a copy button on each code block.
//!
//! The plain-text copy strips the markdown, so a paste into a terminal
//! or an issue carries the words and not the syntax. The code-block
//! copy carries the code exactly as it stands. Both go through egui's
//! copied-text output, which is a plain-text platform fact: the
//! clipboard holds a string, whatever the platform calls the format.

use crate::theme::{FS_XS, Palette, R_XS, SP_2, SP_4};

/// The size of the hover button's icon.
const BUTTON_ICON: f32 = 13.0;

/// One entry's markdown as plain text: the code fences' contents, the
/// headings' and list items' markers and the emphasis marks dropped,
/// the words and the structure kept.
#[must_use]
pub fn plain_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut fence: Option<char> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if fence.is_some() {
            // Inside a code block the line is code, markers included.
            if trimmed.starts_with("```") {
                fence = None;
                continue;
            }
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if trimmed.starts_with("```") {
            fence = Some('`');
            continue;
        }
        // The block markers drop: the heading's hashes, the list
        // item's bullet, the quote's angle and the rule's dashes.
        let body = strip_block_marker(trimmed);
        out.push_str(strip_inline(body).trim_end());
        out.push('\n');
    }
    // A trailing blank line is the last line's newline, not content.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// The block marker of one line dropped: the heading's hashes, the
/// list item's bullet, the quote's angle bracket and the horizontal
/// rule's dashes. The indentation a nested item carries stays, so the
/// structure reads.
fn strip_block_marker(line: &str) -> &str {
    let indent = line.len() - line.trim_start().len();
    let body = &line[indent..];
    let body = body.trim_start_matches('#');
    let body = body.trim_start_matches('>');
    let body = body.trim_start();
    // A list item's bullet, and a task item's checkbox.
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = body.strip_prefix(bullet) {
            return rest.trim_start_matches("[ ] ").trim_start_matches("[x] ");
        }
    }
    // A horizontal rule is dashes with nothing else.
    if !body.is_empty() && body.chars().all(|c| c == '-' || c == '*' || c == '_') {
        return "";
    }
    body
}

/// The inline marks of one line dropped: emphasis, code spans, links
/// (the link text kept, the target dropped) and images' alt text.
fn strip_inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find(['*', '_', '`', '[', '!']) {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        rest = &tail[1..];
        match tail.as_bytes()[0] {
            b'`' => {
                // A code span: its contents, without the backticks.
                if let Some(end) = rest.find('`') {
                    out.push_str(&rest[..end]);
                    rest = &rest[end + 1..];
                } else {
                    out.push('`');
                }
            }
            b'[' => {
                // A link: the text, without the target.
                if let Some(mid) = rest.find("](") {
                    out.push_str(&rest[..mid]);
                    let after = &rest[mid + 2..];
                    if let Some(end) = after.find(')') {
                        rest = &after[end + 1..];
                    } else {
                        out.push('[');
                    }
                } else {
                    out.push('[');
                }
            }
            b'!' if rest.starts_with('[') => {
                // An image: its alt text, without the target.
                let after = &rest[1..];
                if let Some(mid) = after.find("](") {
                    out.push_str(&after[..mid]);
                    let tail = &after[mid + 2..];
                    if let Some(end) = tail.find(')') {
                        rest = &tail[end + 1..];
                    } else {
                        out.push('!');
                    }
                } else {
                    out.push('!');
                }
            }
            _ => {
                // Emphasis: the marks drop, the words stay.
                let marks = rest.chars().take_while(|c| *c == '*' || *c == '_').count();
                rest = &rest[marks.min(3)..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The code blocks of one entry's markdown, in order, each as the
/// lines between its fences.
#[must_use]
pub fn code_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            // An opening fence starts a block, a closing one ends it. A
            // fence carrying a language names the opening one.
            match current.take() {
                Some(block) => blocks.push(block),
                None => current = Some(String::new()),
            }
            continue;
        }
        if let Some(block) = &mut current {
            block.push_str(line);
            block.push('\n');
        }
    }
    // An unclosed fence's block is code too, so it ships.
    if let Some(block) = current {
        blocks.push(block);
    }
    blocks
}

/// Draws the hover affordance that copies `text` as plain text. The
/// button shows while the pointer is over `rect`, and a click copies.
/// The response is the entry's own, so the button never steals it.
pub fn hover_copy(ui: &mut egui::Ui, rect: egui::Rect, text: &str) -> Option<egui::Response> {
    let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
    let hovered = pointer.is_some_and(|at| rect.contains(at));
    if !hovered {
        return None;
    }
    let palette = Palette::active_of(ui.ctx());
    let button = egui::Button::new(
        egui::RichText::new("\u{1F4CB}")
            .size(BUTTON_ICON)
            .color(palette.muted),
    )
    .fill(palette.surface)
    .stroke(egui::Stroke::new(1.0, palette.line))
    .corner_radius(egui::CornerRadius::same(R_XS as u8));
    let response = ui.put(
        egui::Rect::from_min_size(
            egui::pos2(rect.right() - 30.0, rect.top() + SP_4),
            egui::vec2(22.0, 20.0),
        ),
        button,
    );
    if response.clicked() {
        ui.ctx().copy_text(text.to_owned());
    }
    Some(response)
}

/// Draws the copy button of one code block, in the block's top-right
/// corner. A click copies the block exactly as it stands.
pub fn code_copy(ui: &mut egui::Ui, block_rect: egui::Rect, code: &str) -> egui::Response {
    let palette = Palette::active_of(ui.ctx());
    let button = egui::Button::new(egui::RichText::new("copy").size(FS_XS).color(palette.muted))
        .fill(palette.raised)
        .stroke(egui::Stroke::new(1.0, palette.line))
        .corner_radius(egui::CornerRadius::same(R_XS as u8));
    let response = ui.put(
        egui::Rect::from_min_size(
            egui::pos2(block_rect.right() - 46.0 - SP_2, block_rect.top() + SP_2),
            egui::vec2(44.0, 18.0),
        ),
        button,
    );
    if response.clicked() {
        ui.ctx().copy_text(code.to_owned());
    }
    response
}

#[cfg(test)]
mod tests {
    use super::{code_blocks, plain_text};

    #[test]
    fn plain_text_drops_the_markdown_marks_and_keeps_the_words() {
        let markdown = "# A heading\n\nSome **bold** and *italic* and `code`.\n";
        let plain = plain_text(markdown);
        assert!(plain.contains("A heading"), "{plain}");
        assert!(plain.contains("Some bold and italic and code."), "{plain}");
        assert!(!plain.contains('#'), "the heading marker drops");
        assert!(!plain.contains('*'), "the emphasis marks drop");
        assert!(!plain.contains('`'), "the code span's backticks drop");
    }

    #[test]
    fn plain_text_keeps_a_code_block_verbatim() {
        let markdown = "Before.\n\n```rust\nfn main() { let x = 1; }\n```\n\nAfter.\n";
        let plain = plain_text(markdown);
        assert!(
            plain.contains("fn main() { let x = 1; }"),
            "the code keeps its syntax: {plain}"
        );
        assert!(!plain.contains("```"), "the fences drop");
        assert!(plain.contains("Before.") && plain.contains("After."));
    }

    #[test]
    fn plain_text_keeps_a_links_text_and_drops_its_target() {
        let plain = plain_text("See [the docs](https://example.com/docs) now.\n");
        assert!(plain.contains("See the docs now."), "{plain}");
        assert!(!plain.contains("example.com"), "the target drops");
    }

    #[test]
    fn a_list_keeps_its_items() {
        let plain = plain_text("- one\n- two\n  - nested\n");
        assert!(plain.contains("one"), "{plain}");
        assert!(plain.contains("two"), "{plain}");
        assert!(plain.contains("nested"), "{plain}");
    }

    #[test]
    fn code_blocks_come_out_in_order() {
        let markdown = "```\nfirst\n```\n\ntext\n\n```rust\nsecond\n```\n";
        assert_eq!(code_blocks(markdown), vec!["first\n", "second\n"]);
        // A block left open by a cut-off document still ships its code.
        assert_eq!(code_blocks("```\ncut off"), vec!["cut off\n"]);
        assert!(code_blocks("no code here").is_empty());
    }
}
