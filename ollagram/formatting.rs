#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Ordered(u64),
    Unordered,
}

pub const TELEGRAM_MESSAGE_LIMIT: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ListMarker {
    kind: ListKind,
    content_start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenHtmlTag {
    open: String,
    close: String,
}

struct TelegramHtmlWriter {
    chunks: Vec<String>,
    chunk: String,
    open_tags: Vec<OpenHtmlTag>,
    visible_len: usize,
    limit: usize,
}

impl TelegramHtmlWriter {
    fn new(limit: usize) -> Self {
        Self {
            chunks: Vec::new(),
            chunk: String::new(),
            open_tags: Vec::new(),
            visible_len: 0,
            limit,
        }
    }

    fn into_chunks(mut self) -> Vec<String> {
        if !self.chunk.is_empty() {
            self.chunks.push(self.chunk);
        }

        self.chunks
    }

    fn push_text(&mut self, text: &str) {
        for char in text.chars() {
            self.push_escaped_char(char);
        }
    }

    fn push_escaped_char(&mut self, char: char) {
        if char == '<' {
            self.push_visible_html("&lt;");
        } else if char == '>' {
            self.push_visible_html("&gt;");
        } else if char == '&' {
            self.push_visible_html("&amp;");
        } else {
            self.push_visible_char(char);
        }
    }

    fn push_visible_char(&mut self, char: char) {
        self.flush_if_full();
        self.chunk.push(char);
        self.visible_len += 1;
    }

    fn push_visible_html(&mut self, html: &str) {
        self.flush_if_full();
        self.chunk.push_str(html);
        self.visible_len += 1;
    }

    fn push_tagged<F>(&mut self, open: String, close: &'static str, write_content: F)
    where
        F: FnOnce(&mut Self),
    {
        self.open_tag(open, close.to_owned());
        write_content(self);
        self.close_tag(close);
    }

    fn open_tag(&mut self, open: String, close: String) {
        self.chunk.push_str(&open);
        self.open_tags.push(OpenHtmlTag { open, close });
    }

    fn close_tag(&mut self, close: &str) {
        if let Some(index) = self.open_tags.iter().rposition(|tag| tag.close == close) {
            self.open_tags.remove(index);
        }

        self.chunk.push_str(close);
    }

    fn flush_if_full(&mut self) {
        if self.visible_len < self.limit {
            return;
        }

        for tag in self.open_tags.iter().rev() {
            self.chunk.push_str(&tag.close);
        }

        self.chunks.push(std::mem::take(&mut self.chunk));

        for tag in &self.open_tags {
            self.chunk.push_str(&tag.open);
        }

        self.visible_len = 0;
    }
}

pub fn markdown_to_telegram_html_chunks(markdown: &str) -> Vec<String> {
    let mut writer = TelegramHtmlWriter::new(TELEGRAM_MESSAGE_LIMIT);
    write_markdown(markdown, &mut writer);
    writer.into_chunks()
}

fn write_markdown(markdown: &str, writer: &mut TelegramHtmlWriter) {
    let lines = markdown.lines().collect::<Vec<_>>();
    let mut line_index = 0;

    while let Some(line) = lines.get(line_index).copied() {
        if line.trim().is_empty() {
            writer.push_text("\n");
            line_index += 1;
            continue;
        }

        if let Some(language) = fenced_code_language(line) {
            let (code, next_line_index) = collect_fenced_code(&lines, line_index + 1);
            write_pre(&code, language, writer);
            push_block_separator(writer, &lines, next_line_index);
            line_index = next_line_index;
            continue;
        }

        if is_blockquote(line) {
            let next_line_index = write_blockquote(&lines, line_index, writer);
            push_block_separator(writer, &lines, next_line_index);
            line_index = next_line_index;
            continue;
        }

        if let Some(marker) = list_marker(line) {
            let next_line_index = write_list(&lines, line_index, marker.kind, writer);
            push_block_separator(writer, &lines, next_line_index);
            line_index = next_line_index;
            continue;
        }

        if let Some(heading) = heading_text(line) {
            writer.push_tagged(String::from("<b>"), "</b>", |writer| {
                write_inline_markdown(heading, writer);
            });
            push_block_separator(writer, &lines, line_index + 1);
            line_index += 1;
            continue;
        }

        let next_line_index = write_paragraph(&lines, line_index, writer);
        push_block_separator(writer, &lines, next_line_index);
        line_index = next_line_index;
    }
}

fn push_block_separator(writer: &mut TelegramHtmlWriter, lines: &[&str], next_line_index: usize) {
    if lines.get(next_line_index).is_some() {
        writer.push_text("\n");
    }
}

fn collect_fenced_code(lines: &[&str], start_index: usize) -> (String, usize) {
    let mut code = String::new();
    let mut line_index = start_index;

    while let Some(line) = lines.get(line_index).copied() {
        if line.trim_start().starts_with("```") {
            return (code.trim_end_matches('\n').to_owned(), line_index + 1);
        }

        code.push_str(line);
        code.push('\n');
        line_index += 1;
    }

    (code.trim_end_matches('\n').to_owned(), line_index)
}

fn write_blockquote(lines: &[&str], start_index: usize, writer: &mut TelegramHtmlWriter) -> usize {
    writer.open_tag(String::from("<blockquote>"), String::from("</blockquote>"));

    let mut line_index = start_index;
    let mut is_first_line = true;

    while let Some(line) = lines.get(line_index).copied() {
        if !is_blockquote(line) {
            writer.close_tag("</blockquote>");
            return line_index;
        }

        if is_first_line {
            is_first_line = false;
        } else {
            writer.push_text("\n");
        }

        let content = line
            .trim_start()
            .strip_prefix('>')
            .map(str::trim_start)
            .unwrap_or("");
        write_inline_markdown(content, writer);
        line_index += 1;
    }

    writer.close_tag("</blockquote>");
    line_index
}

fn write_list(
    lines: &[&str],
    start_index: usize,
    kind: ListKind,
    writer: &mut TelegramHtmlWriter,
) -> usize {
    let mut line_index = start_index;
    let mut number = match kind {
        ListKind::Ordered(start) => start,
        ListKind::Unordered => 1,
    };
    let mut is_first_item = true;

    while let Some(line) = lines.get(line_index).copied() {
        let Some(marker) = list_marker(line) else {
            return line_index;
        };

        if !same_list_kind(kind, marker.kind) {
            return line_index;
        }

        if is_first_item {
            is_first_item = false;
        } else {
            writer.push_text("\n");
        }

        match kind {
            ListKind::Ordered(_) => {
                writer.push_text(&format!("{number}. "));
                number += 1;
            }
            ListKind::Unordered => writer.push_text("- "),
        }

        write_inline_markdown(&line[marker.content_start..], writer);
        line_index += 1;
    }

    line_index
}

fn write_paragraph(lines: &[&str], start_index: usize, writer: &mut TelegramHtmlWriter) -> usize {
    let mut line_index = start_index;
    let mut is_first_line = true;

    while let Some(line) = lines.get(line_index).copied() {
        if line.trim().is_empty()
            || fenced_code_language(line).is_some()
            || is_blockquote(line)
            || list_marker(line).is_some()
            || heading_text(line).is_some()
        {
            return line_index;
        }

        if is_first_line {
            is_first_line = false;
        } else {
            writer.push_text("\n");
        }

        write_inline_markdown(line, writer);
        line_index += 1;
    }

    line_index
}

fn write_inline_markdown(markdown: &str, writer: &mut TelegramHtmlWriter) {
    let chars = markdown.chars().collect::<Vec<_>>();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] == '`'
            && let Some(end) = find_char(&chars, index + 1, '`')
        {
            writer.push_tagged(String::from("<code>"), "</code>", |writer| {
                writer.push_text(&chars[index + 1..end].iter().collect::<String>());
            });
            index = end + 1;
            continue;
        }

        if let Some((tag, delimiter_len, content, next_index)) = inline_delimited(&chars, index) {
            if let Some(close) = tag_close(tag) {
                writer.push_tagged(format!("<{tag}>"), close, |writer| {
                    write_inline_markdown(&content, writer);
                });
            }
            index = next_index + delimiter_len;
            continue;
        }

        if let Some((label, href, next_index)) = inline_link(&chars, index) {
            writer.push_tagged(
                format!("<a href=\"{}\">", escape_attribute(&href)),
                "</a>",
                |writer| {
                    write_inline_markdown(&label, writer);
                },
            );
            index = next_index;
            continue;
        }

        writer.push_escaped_char(chars[index]);
        index += 1;
    }
}

fn tag_close(tag: &str) -> Option<&'static str> {
    if tag == "b" {
        Some("</b>")
    } else if tag == "i" {
        Some("</i>")
    } else if tag == "s" {
        Some("</s>")
    } else {
        None
    }
}

fn inline_delimited(chars: &[char], index: usize) -> Option<(&'static str, usize, String, usize)> {
    let candidates = [
        ("**", "b", 2),
        ("__", "b", 2),
        ("~~", "s", 2),
        ("*", "i", 1),
        ("_", "i", 1),
    ];

    candidates.iter().find_map(|(delimiter, tag, len)| {
        starts_with(chars, index, delimiter)
            .then(|| find_delimiter(chars, index + len, delimiter))
            .flatten()
            .map(|end| {
                (
                    *tag,
                    *len,
                    chars[index + len..end].iter().collect::<String>(),
                    end,
                )
            })
    })
}

fn inline_link(chars: &[char], index: usize) -> Option<(String, String, usize)> {
    let label_start = if starts_with(chars, index, "![") {
        index + 2
    } else if chars.get(index) == Some(&'[') {
        index + 1
    } else {
        return None;
    };

    let label_end = find_char(chars, label_start, ']')?;
    if chars.get(label_end + 1) != Some(&'(') {
        return None;
    }

    let href_end = find_char(chars, label_end + 2, ')')?;
    let label = chars[label_start..label_end].iter().collect::<String>();
    let href = chars[label_end + 2..href_end].iter().collect::<String>();

    Some((label, href, href_end + 1))
}

fn write_pre(code: &str, language: Option<&str>, writer: &mut TelegramHtmlWriter) {
    match language.and_then(language_class) {
        Some(language) => {
            writer.push_tagged(
                format!(
                    "<pre><code class=\"language-{}\">",
                    escape_attribute(language)
                ),
                "</code></pre>",
                |writer| {
                    writer.push_text(code);
                },
            );
        }
        None => {
            writer.push_tagged(String::from("<pre>"), "</pre>", |writer| {
                writer.push_text(code);
            });
        }
    }
}

fn fenced_code_language(line: &str) -> Option<Option<&str>> {
    let trimmed = line.trim_start();
    let language = trimmed.strip_prefix("```")?.trim();

    if language.is_empty() {
        Some(None)
    } else {
        Some(Some(language))
    }
}

fn language_class(language: &str) -> Option<&str> {
    language
        .chars()
        .all(|char| char.is_ascii_alphanumeric() || char == '-' || char == '_')
        .then_some(language)
}

fn is_blockquote(line: &str) -> bool {
    line.trim_start().starts_with('>')
}

fn heading_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|char| *char == '#').count();

    if (1..=6).contains(&hashes) && trimmed.chars().nth(hashes) == Some(' ') {
        return Some(trimmed[hashes + 1..].trim());
    }

    None
}

fn list_marker(line: &str) -> Option<ListMarker> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let mut chars = trimmed.char_indices();
    let first = chars.next()?.1;

    if matches!(first, '-' | '*' | '+') && chars.next()?.1 == ' ' {
        return Some(ListMarker {
            kind: ListKind::Unordered,
            content_start: indent + 2,
        });
    }

    if !first.is_ascii_digit() {
        return None;
    }

    let marker_end = trimmed
        .char_indices()
        .find_map(|(index, char)| (!char.is_ascii_digit()).then_some((index, char)))?;

    let (index, separator) = marker_end;
    if separator != '.' || trimmed.chars().nth(index + 1) != Some(' ') {
        return None;
    }

    let start = trimmed[..index].parse::<u64>().ok()?;
    Some(ListMarker {
        kind: ListKind::Ordered(start),
        content_start: indent + index + 2,
    })
}

fn same_list_kind(expected: ListKind, actual: ListKind) -> bool {
    match (expected, actual) {
        (ListKind::Ordered(_), ListKind::Ordered(_)) => true,
        (ListKind::Unordered, ListKind::Unordered) => true,
        (ListKind::Ordered(_), ListKind::Unordered) => false,
        (ListKind::Unordered, ListKind::Ordered(_)) => false,
    }
}

fn starts_with(chars: &[char], index: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(offset, char)| chars.get(index + offset) == Some(&char))
}

fn find_delimiter(chars: &[char], start: usize, delimiter: &str) -> Option<usize> {
    (start..chars.len()).find(|index| starts_with(chars, *index, delimiter))
}

fn find_char(chars: &[char], start: usize, needle: char) -> Option<usize> {
    chars
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, char)| (*char == needle).then_some(index))
}

fn escape_attribute(text: &str) -> String {
    text.chars().fold(String::new(), |mut escaped, char| {
        if char == '<' {
            escaped.push_str("&lt;");
        } else if char == '>' {
            escaped.push_str("&gt;");
        } else if char == '&' {
            escaped.push_str("&amp;");
        } else if char == '"' {
            escaped.push_str("&quot;");
        } else {
            escaped.push(char);
        }

        escaped
    })
}

#[cfg(test)]
mod tests {
    use super::{TELEGRAM_MESSAGE_LIMIT, TelegramHtmlWriter, markdown_to_telegram_html_chunks};

    fn markdown_to_telegram_html(markdown: &str) -> String {
        markdown_to_telegram_html_chunks(markdown).join("")
    }

    #[test]
    fn formats_common_llm_markdown_as_telegram_html() {
        let markdown = "# Plan\n- **Ship** code\n- Use `Result<T>`\n\n```rust\nfn main() {}\n```\n";

        let html = markdown_to_telegram_html(markdown);

        assert_eq!(
            html,
            "<b>Plan</b>\n- <b>Ship</b> code\n- Use <code>Result&lt;T&gt;</code>\n\n<pre><code class=\"language-rust\">fn main() {}</code></pre>"
        );
    }

    #[test]
    fn escapes_unsupported_html() {
        let markdown = "Hello <div>x & y</div>";

        let html = markdown_to_telegram_html(markdown);

        assert_eq!(html, "Hello &lt;div&gt;x &amp; y&lt;/div&gt;");
    }

    #[test]
    fn formats_links_and_quotes() {
        let markdown = "> See [docs](https://example.com?a=1&b=2)\n> ~~soon~~";

        let html = markdown_to_telegram_html(markdown);

        assert_eq!(
            html,
            "<blockquote>See <a href=\"https://example.com?a=1&amp;b=2\">docs</a>\n<s>soon</s></blockquote>"
        );
    }

    #[test]
    fn chunks_formatted_html_without_exceeding_visible_limit() {
        let markdown = "x".repeat(TELEGRAM_MESSAGE_LIMIT + 1);

        let chunks = markdown_to_telegram_html_chunks(&markdown);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), TELEGRAM_MESSAGE_LIMIT);
        assert_eq!(chunks[1], "x");
    }

    #[test]
    fn chunks_keep_open_tags_valid() {
        let markdown = format!("**{}**", "x".repeat(TELEGRAM_MESSAGE_LIMIT + 1));

        let chunks = markdown_to_telegram_html_chunks(&markdown);

        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].starts_with("<b>"));
        assert!(chunks[0].ends_with("</b>"));
        assert_eq!(chunks[1], "<b>x</b>");
    }

    #[test]
    fn chunks_count_entities_as_visible_characters() {
        let markdown = "<".repeat(TELEGRAM_MESSAGE_LIMIT + 1);

        let chunks = markdown_to_telegram_html_chunks(&markdown);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "&lt;".repeat(TELEGRAM_MESSAGE_LIMIT));
        assert_eq!(chunks[1], "&lt;");
    }

    #[test]
    fn chunk_writer_does_not_parse_generated_html() {
        let mut writer = TelegramHtmlWriter::new(1);

        writer.push_tagged(String::from("<b>"), "</b>", |writer| {
            writer.push_text("ab");
        });

        assert_eq!(writer.into_chunks(), vec!["<b>a</b>", "<b>b</b>"]);
    }
}
