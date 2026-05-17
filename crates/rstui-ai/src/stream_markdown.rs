//! `stream_markdown` — a streaming-markdown view: incomplete-markdown
//! *repair* + block segmentation + a per-block render cache, projected
//! through the existing [`rstui_widgets::Markdown`] /
//! [`rstui_widgets::Mermaid`] renderers.
//!
//! # The problem this solves
//!
//! [`rstui_widgets::Markdown`] is a complete-document renderer: hand it a
//! finished CommonMark-ish string and it lays out headings, emphasis,
//! code, lists, tables. An *agent* does not produce a finished string —
//! it streams one token at a time, so at any instant the buffer is a
//! *prefix* of valid markdown: `This is **bol` (an open bold run),
//! `[the docs](http` (an open link), ```` ```py ```` then code with no
//! closing fence. Rendering that prefix verbatim shows raw `**`, mode
//! flips when the closing marker finally arrives, and a flicker on every
//! token. The fix (the approach Vercel's
//! [streamdown](https://github.com/vercel/streamdown) `remend` takes) is
//! to *repair* the prefix into the smallest valid document that renders
//! the way the finished one will: close the open `**`, swap the open link
//! for a placeholder, treat the unterminated fence as a plain code block.
//!
//! # Three pieces, each a pure function
//!
//! 1. [`remend`] — the repair. A fixed-priority pipeline of
//!    [handlers](RemendHandler): escape a lone `~`, escape a list-item
//!    comparison `>`, strip a half-typed HTML tag, break a streamed
//!    bullet that looks like a setext underline, close/placeholder an open
//!    link or image, then balance the emphasis/code/strikethrough/KaTeX
//!    markers in a deliberate nesting order. Every rule is a faithful
//!    port of the streamdown handler of the same name, including its
//!    immunities: nothing is repaired inside a complete inline-code span
//!    or a fenced block, a standalone marker (`**` alone) is never
//!    closed, a word-internal `*` (`hello*world`) and a whitespace-flanked
//!    `*` (`5 * 0`) are never emphasis. [`remend`] is idempotent:
//!    `remend(remend(x)) == remend(x)`.
//! 2. [`parse_into_blocks`] — split the repaired document into top-level
//!    block source strings, keeping an unterminated fenced/`$$`/HTML block
//!    (and a footnote-bearing doc) whole so a half-arrived block is one
//!    segment, not a torn pair.
//! 3. [`StreamMarkdown`] — the widget: a pure projection of a
//!    caller-owned `&str` source and a caller-owned [`StreamMarkdownState`]
//!    (which holds the per-block [`StreamCache`]). It is
//!    [`remend`] → [`parse_into_blocks`] → render each block through
//!    [`rstui_widgets::Markdown`], reusing cached lines for blocks whose
//!    source string is unchanged so an earlier settled block is never
//!    re-laid-out — only the changing tail re-renders.
//!
//! # Discipline (ADR 0012, `docs/composition.md`)
//!
//! The widget owns no state; state lives in [`StreamMarkdownState`], the
//! model owns it, `view` reads it, and `update` mutates it via the
//! explicit [`StreamMarkdownState::ingest`] step — render
//! ([`StreamMarkdown::lines`] / [`Widget::render`]) is pure and never
//! mutates the cache. Every function here is *total*: empty, huge,
//! truncated, or hostile markdown and a `0×0` area all clip or no-op,
//! never panic, never `unwrap()` on the input. The scanners are strictly
//! linear-time over char indices (no backtracking, no `O(n²)` rescans of
//! the growing buffer) because [`remend`] runs on *every* streamed token.

use std::borrow::Cow;

use rstui_core::{Buffer, Line, Rect, Widget};
use rstui_widgets::{Markdown, Mermaid};

/// How [`remend`] repairs an incomplete `[text](url` link.
///
/// Mirrors streamdown's `linkMode`. An incomplete *image* (`![alt`) is
/// always deleted regardless of this mode — a terminal cannot show a
/// skeleton bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkMode {
    /// Keep the link text and close it with the placeholder URL
    /// `streamdown:incomplete-link` (the default, matching streamdown):
    /// `[the docs](htt` → `[the docs](streamdown:incomplete-link)`.
    #[default]
    Protocol,
    /// Drop the link markup and display only the bare text:
    /// `[the docs](htt` → `the docs`.
    TextOnly,
}

/// The placeholder URL [`LinkMode::Protocol`] closes an incomplete link
/// with — the exact streamdown sentinel, so a downstream renderer can
/// recognise and style it.
pub const INCOMPLETE_LINK_URL: &str = "streamdown:incomplete-link";

/// A caller-supplied extra repair pass, run after the built-ins.
///
/// The streamdown `RemendHandler` shape: a `name` (for debugging), a
/// `priority` (lower runs first; the built-ins occupy `0..=75`, the
/// default is [`DEFAULT_HANDLER_PRIORITY`]), and the transform itself.
/// The transform is a plain `fn(&str) -> String` pointer (not a boxed
/// closure) so [`RemendOptions`] stays `Clone` and allocation-free to
/// pass around — a custom rule is pure text→text, exactly like a
/// built-in.
#[derive(Clone)]
pub struct RemendHandler {
    /// A stable identifier for this handler (diagnostics only).
    pub name: &'static str,
    /// Lower runs first. Built-ins are `0..=75`; the conventional
    /// default for a custom handler is [`DEFAULT_HANDLER_PRIORITY`].
    pub priority: i32,
    /// The repair: receives the in-progress text, returns the repaired
    /// text. Must be total (panic-free for every input) and ideally
    /// idempotent so the whole pipeline stays idempotent.
    pub transform: fn(&str) -> String,
}

impl core::fmt::Debug for RemendHandler {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("RemendHandler")
            .field("name", &self.name)
            .field("priority", &self.priority)
            .finish_non_exhaustive()
    }
}

/// The conventional priority of a custom [`RemendHandler`] — runs after
/// every built-in (which all sit at `<= 75`), the streamdown default.
pub const DEFAULT_HANDLER_PRIORITY: i32 = 100;

/// Which incomplete-markdown repairs [`remend`] performs.
///
/// One flag per streamdown handler. [`RemendOptions::default`] matches
/// streamdown's defaults exactly: **everything on except
/// [`inline_katex`](Self::inline_katex)** (a lone `$` is ambiguous with
/// currency, so inline-KaTeX completion is opt-in), and
/// [`link_mode`](Self::link_mode) [`Protocol`](LinkMode::Protocol).
#[derive(Clone, Debug)]
pub struct RemendOptions {
    /// Complete `**bold` → `**bold**` (priority 35).
    pub bold: bool,
    /// Complete `***both` → `***both***` (priority 30).
    pub bold_italic: bool,
    /// Escape a list-item comparison `>` so it is not read as a
    /// blockquote: `- > 25` → `- \> 25` (priority 5).
    pub comparison_operators: bool,
    /// Strip a half-typed trailing HTML tag: `text <cust` → `text`
    /// (priority 10).
    pub html_tags: bool,
    /// Delete an incomplete image `![alt` (priority 20, shared handler).
    pub images: bool,
    /// Complete `` `code `` → `` `code` `` (priority 50).
    pub inline_code: bool,
    /// Complete a lone `$inline` → `$inline$`. **Off by default** (a
    /// single `$` is usually currency) — opt in explicitly (priority 75).
    pub inline_katex: bool,
    /// Complete `*it` / `_it` / `__it` italics (priorities 40/41/42).
    pub italic: bool,
    /// Complete block `$$x` → `$$x$$` (priority 70).
    pub katex: bool,
    /// How to repair an incomplete `[text](url` link.
    pub link_mode: LinkMode,
    /// Complete/placeholder an incomplete link (priority 20).
    pub links: bool,
    /// Break a streamed bullet that looks like a setext underline:
    /// `text\n-` → `text\n-\u{200b}` (priority 15).
    pub setext_headings: bool,
    /// Escape a lone word-internal `~`: `20~25` → `20\~25` (priority 0).
    pub single_tilde: bool,
    /// Complete `~~strike` → `~~strike~~` (priority 60).
    pub strikethrough: bool,
    /// Extra caller passes, run after the built-ins (see
    /// [`RemendHandler`]).
    pub handlers: Vec<RemendHandler>,
}

impl Default for RemendOptions {
    fn default() -> Self {
        Self {
            bold: true,
            bold_italic: true,
            comparison_operators: true,
            html_tags: true,
            images: true,
            inline_code: true,
            inline_katex: false,
            italic: true,
            katex: true,
            link_mode: LinkMode::Protocol,
            links: true,
            setext_headings: true,
            single_tilde: true,
            strikethrough: true,
            handlers: Vec::new(),
        }
    }
}

// ===========================================================================
// Char-index scanners (linear time, Unicode-correct, total).
//
// streamdown indexes a JS string by UTF-16 code unit; we operate on a
// `&[char]` slice built once per `remend` call, so every index below is a
// char index and every scan is a single forward/backward pass. None of
// these allocate or rescan a growing prefix quadratically.
// ===========================================================================

/// A word char per CommonMark emphasis flanking: a Unicode letter or
/// number, or `_` (the ASCII fast path is implicit in `is_alphanumeric`).
fn is_word_char(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

/// `true` if `chars[..at]` leaves us inside an *odd* run of ```` ``` ````
/// fences — i.e. char index `at` is inside a fenced code block (the
/// streamdown `isWithinCodeBlock`: triple backticks toggle, others
/// ignored).
fn within_fenced_block(chars: &[char], at: usize) -> bool {
    let mut inside = false;
    let mut index = 0;
    while index < at && index < chars.len() {
        if chars[index] == '`'
            && chars.get(index + 1) == Some(&'`')
            && chars.get(index + 2) == Some(&'`')
        {
            inside = !inside;
            index += 3;
            continue;
        }
        index += 1;
    }
    inside
}

/// streamdown's `isInsideCodeBlock`: like [`within_fenced_block`] but a
/// *single* backtick (not part of a ```` ``` ````) also toggles an inline
/// span, and `\``  is skipped. An incomplete span counts as "inside".
fn within_code_span_or_fence(chars: &[char], at: usize) -> bool {
    let mut in_inline = false;
    let mut in_fence = false;
    let mut index = 0;
    while index < at && index < chars.len() {
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'`') {
            index += 2;
            continue;
        }
        if chars[index] == '`'
            && chars.get(index + 1) == Some(&'`')
            && chars.get(index + 2) == Some(&'`')
        {
            in_fence = !in_fence;
            index += 3;
            continue;
        }
        if !in_fence && chars[index] == '`' {
            in_inline = !in_inline;
        }
        index += 1;
    }
    in_inline || in_fence
}

/// streamdown's `isWithinCompleteInlineCode`: `true` only when `at` lies
/// strictly between a *matched* pair of single backticks (an unterminated
/// span — the streaming case — returns `false` so emphasis can still be
/// repaired). Fenced blocks are skipped wholesale.
fn within_complete_inline_code(chars: &[char], at: usize) -> bool {
    let mut in_inline = false;
    let mut in_fence = false;
    let mut span_start: isize = -1;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'`') {
            index += 2;
            continue;
        }
        if chars[index] == '`'
            && chars.get(index + 1) == Some(&'`')
            && chars.get(index + 2) == Some(&'`')
        {
            in_fence = !in_fence;
            index += 3;
            continue;
        }
        if !in_fence && chars[index] == '`' {
            if in_inline {
                let here = at as isize;
                if span_start < here && here < index as isize {
                    return true;
                }
                in_inline = false;
                span_start = -1;
            } else {
                in_inline = true;
                span_start = index as isize;
            }
        }
        index += 1;
    }
    false
}

/// streamdown's `isWithinMathBlock`: count `$`/`$$` toggles (block math
/// takes precedence over inline; `\$` is skipped) up to `at`.
fn within_math_block(chars: &[char], at: usize) -> bool {
    let mut in_inline = false;
    let mut in_block = false;
    let mut index = 0;
    while index < chars.len() && index < at {
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'$') {
            index += 2;
            continue;
        }
        if chars[index] == '$' {
            if chars.get(index + 1) == Some(&'$') {
                in_block = !in_block;
                index += 1;
                in_inline = false;
            } else if !in_block {
                in_inline = !in_inline;
            }
        }
        index += 1;
    }
    in_inline || in_block
}

/// Is there a `)` after `from` before the next `\n`? (streamdown's
/// `isBeforeClosingParen`.)
fn closing_paren_on_line(chars: &[char], from: usize) -> bool {
    let mut index = from;
    while index < chars.len() {
        match chars[index] {
            ')' => return true,
            '\n' => return false,
            _ => index += 1,
        }
    }
    false
}

/// streamdown's `isWithinLinkOrImageUrl`: walking back from `at`, a `(`
/// immediately preceded by `]` (and a `)` still ahead on the line) means
/// `at` is inside a `[…](…` URL.
fn within_link_or_image_url(chars: &[char], at: usize) -> bool {
    let mut index = at as isize - 1;
    while index >= 0 {
        let here = index as usize;
        match chars[here] {
            ')' => return false,
            '(' => {
                if here > 0 && chars[here - 1] == ']' {
                    return closing_paren_on_line(chars, at);
                }
                return false;
            }
            '\n' => return false,
            _ => index -= 1,
        }
    }
    false
}

/// streamdown's `isWithinHtmlTag`: walking back from `at`, an unclosed
/// `<` that starts a tag (next char a letter or `/`) means `at` is inside
/// the tag (e.g. the `_` in `<a target="_blank">`).
fn within_html_tag(chars: &[char], at: usize) -> bool {
    let mut index = at as isize - 1;
    while index >= 0 {
        let here = index as usize;
        match chars[here] {
            '>' => return false,
            '<' => {
                let next = chars.get(here + 1).copied().unwrap_or('\0');
                return next.is_ascii_alphabetic() || next == '/';
            }
            '\n' => return false,
            _ => index -= 1,
        }
    }
    false
}

/// streamdown's `isHorizontalRule`: the line containing `marker_index` is
/// nothing but `>= 3` copies of `marker` and optional spaces/tabs.
fn is_horizontal_rule(chars: &[char], marker_index: usize, marker: char) -> bool {
    let mut line_start = 0;
    let mut scan = marker_index as isize - 1;
    while scan >= 0 {
        if chars[scan as usize] == '\n' {
            line_start = scan as usize + 1;
            break;
        }
        scan -= 1;
    }
    let mut line_end = chars.len();
    let mut forward = marker_index;
    while forward < chars.len() {
        if chars[forward] == '\n' {
            line_end = forward;
            break;
        }
        forward += 1;
    }
    let mut marker_count = 0;
    for &character in &chars[line_start..line_end] {
        if character == marker {
            marker_count += 1;
        } else if character != ' ' && character != '\t' {
            return false;
        }
    }
    marker_count >= 3
}

/// streamdown's `findMatchingOpeningBracket`: the `[` that balances the
/// `]` at `close_index`, accounting for nesting (or `None`).
fn matching_opening_bracket(chars: &[char], close_index: usize) -> Option<usize> {
    let mut depth = 1;
    let mut index = close_index as isize - 1;
    while index >= 0 {
        let here = index as usize;
        if chars[here] == ']' {
            depth += 1;
        } else if chars[here] == '[' {
            depth -= 1;
            if depth == 0 {
                return Some(here);
            }
        }
        index -= 1;
    }
    None
}

/// streamdown's `findMatchingClosingBracket`: the `]` that balances the
/// `[` at `open_index`, accounting for nesting (or `None`).
fn matching_closing_bracket(chars: &[char], open_index: usize) -> Option<usize> {
    let mut depth = 1;
    let mut index = open_index + 1;
    while index < chars.len() {
        if chars[index] == '[' {
            depth += 1;
        } else if chars[index] == ']' {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

/// Char index of the last occurrence of `needle` in `chars` (streamdown
/// leans on JS `lastIndexOf` to locate the matched marker).
fn last_index_of(chars: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > chars.len() {
        return None;
    }
    let mut start = chars.len() - needle.len();
    loop {
        if chars[start..start + needle.len()] == *needle {
            return Some(start);
        }
        if start == 0 {
            return None;
        }
        start -= 1;
    }
}

/// `^[\s_~*`]*$` — content that is only whitespace / emphasis markers, so
/// there is nothing to actually emphasise (streamdown's
/// `whitespaceOrMarkersPattern`). An empty tail also qualifies.
fn is_only_whitespace_or_markers(tail: &[char]) -> bool {
    tail.iter()
        .all(|character| character.is_whitespace() || matches!(character, '_' | '~' | '*' | '`'))
}

/// `^[\s]*[-*+][\s]+$` — a bare bullet marker with trailing space and no
/// content yet (streamdown's `listItemPattern`).
fn is_bare_list_marker_line(line: &[char]) -> bool {
    let mut index = 0;
    while index < line.len() && line[index].is_whitespace() {
        index += 1;
    }
    if index >= line.len() || !matches!(line[index], '-' | '*' | '+') {
        return false;
    }
    index += 1;
    if index >= line.len() || !line[index].is_whitespace() {
        return false;
    }
    line[index..].iter().all(|c| c.is_whitespace())
}

// ===========================================================================
// Marker counters (streamdown's count* family) — single forward pass each.
// ===========================================================================

/// Does `chars` end with `>= 4` asterisks and *nothing else*?
/// (streamdown's `fourOrMoreAsterisksPattern` `^\*{4,}$`.)
fn is_four_or_more_asterisks_only(chars: &[char]) -> bool {
    chars.len() >= 4 && chars.iter().all(|&c| c == '*')
}

/// `**` pairs outside fenced code blocks.
fn double_asterisk_pairs(chars: &[char]) -> usize {
    let mut count = 0;
    let mut in_fence = false;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`'
            && chars.get(index + 1) == Some(&'`')
            && chars.get(index + 2) == Some(&'`')
        {
            in_fence = !in_fence;
            index += 3;
            continue;
        }
        if in_fence {
            index += 1;
            continue;
        }
        if chars[index] == '*' && chars.get(index + 1) == Some(&'*') {
            count += 1;
            index += 2;
            continue;
        }
        index += 1;
    }
    count
}

/// `__` pairs outside fenced code blocks.
fn double_underscore_pairs(chars: &[char]) -> usize {
    let mut count = 0;
    let mut in_fence = false;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`'
            && chars.get(index + 1) == Some(&'`')
            && chars.get(index + 2) == Some(&'`')
        {
            in_fence = !in_fence;
            index += 3;
            continue;
        }
        if in_fence {
            index += 1;
            continue;
        }
        if chars[index] == '_' && chars.get(index + 1) == Some(&'_') {
            count += 1;
            index += 2;
            continue;
        }
        index += 1;
    }
    count
}

/// `~~` pairs (streamdown counts these with a global `/~~/g`, code-block
/// agnostic — matched here for fidelity).
fn double_tilde_pairs(chars: &[char]) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index + 1 < chars.len() {
        if chars[index] == '~' && chars[index + 1] == '~' {
            count += 1;
            index += 2;
            continue;
        }
        index += 1;
    }
    count
}

/// Should the `*` at `index` be *skipped* when counting single asterisks
/// (streamdown's `shouldSkipAsterisk`)? Handles escape, math, the special
/// first-`*`-of-`***` case, `**`-membership, word-internal, and
/// whitespace-flanked immunities.
fn should_skip_asterisk(chars: &[char], index: usize, has_math: bool) -> bool {
    let prev = if index > 0 { chars[index - 1] } else { '\0' };
    let next = chars.get(index + 1).copied().unwrap_or('\0');

    if prev == '\\' {
        return true;
    }
    if has_math && within_math_block(chars, index) {
        return true;
    }
    if prev != '*' && next == '*' {
        let next_next = chars.get(index + 2).copied().unwrap_or('\0');
        if next_next == '*' {
            // First `*` of a `***` run — counts as a single (can close `*`).
            return false;
        }
        // First `*` of a `**` (not `***`).
        return true;
    }
    if prev == '*' {
        return true;
    }
    if prev != '\0' && next != '\0' && is_word_char(prev) && is_word_char(next) {
        return true;
    }
    let prev_ws = prev == '\0' || prev == ' ' || prev == '\t' || prev == '\n';
    let next_ws = next == '\0' || next == ' ' || next == '\t' || next == '\n';
    prev_ws && next_ws
}

/// Single asterisks that are real emphasis delimiters (streamdown's
/// `countSingleAsterisks`, fenced-block aware).
fn count_single_asterisks(chars: &[char]) -> usize {
    let has_math = chars.contains(&'$');
    let mut count = 0;
    let mut in_fence = false;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`'
            && index + 2 < chars.len()
            && chars[index + 1] == '`'
            && chars[index + 2] == '`'
        {
            in_fence = !in_fence;
            index += 3;
            continue;
        }
        if in_fence {
            index += 1;
            continue;
        }
        if chars[index] == '*' && !should_skip_asterisk(chars, index, has_math) {
            count += 1;
        }
        index += 1;
    }
    count
}

/// Should the `_` at `index` be skipped when counting single underscores
/// (streamdown's `shouldSkipUnderscore`)?
fn should_skip_underscore(chars: &[char], index: usize, has_math: bool) -> bool {
    let prev = if index > 0 { chars[index - 1] } else { '\0' };
    let next = chars.get(index + 1).copied().unwrap_or('\0');

    if prev == '\\' {
        return true;
    }
    if has_math && within_math_block(chars, index) {
        return true;
    }
    if within_link_or_image_url(chars, index) {
        return true;
    }
    if within_html_tag(chars, index) {
        return true;
    }
    if prev == '_' || next == '_' {
        return true;
    }
    prev != '\0' && next != '\0' && is_word_char(prev) && is_word_char(next)
}

/// Single underscores that are real emphasis delimiters (streamdown's
/// `countSingleUnderscores`, fenced-block aware).
fn count_single_underscores(chars: &[char]) -> usize {
    let has_math = chars.contains(&'$');
    let mut count = 0;
    let mut in_fence = false;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`'
            && index + 2 < chars.len()
            && chars[index + 1] == '`'
            && chars[index + 2] == '`'
        {
            in_fence = !in_fence;
            index += 3;
            continue;
        }
        if in_fence {
            index += 1;
            continue;
        }
        if chars[index] == '_' && !should_skip_underscore(chars, index, has_math) {
            count += 1;
        }
        index += 1;
    }
    count
}

/// `***`-and-longer groups, `floor(run/3)` each, outside fenced blocks
/// (streamdown's `countTripleAsterisks`).
fn count_triple_asterisks(chars: &[char]) -> usize {
    let mut count = 0;
    let mut run = 0;
    let mut in_fence = false;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`'
            && index + 2 < chars.len()
            && chars[index + 1] == '`'
            && chars[index + 2] == '`'
        {
            if run >= 3 {
                count += run / 3;
            }
            run = 0;
            in_fence = !in_fence;
            index += 3;
            continue;
        }
        if in_fence {
            index += 1;
            continue;
        }
        if chars[index] == '*' {
            run += 1;
        } else {
            if run >= 3 {
                count += run / 3;
            }
            run = 0;
        }
        index += 1;
    }
    if run >= 3 {
        count += run / 3;
    }
    count
}

/// Is the backtick at `index` part of any ```` ``` ```` (streamdown's
/// `isTripleBacktick` for the KaTeX counters)?
fn is_part_of_triple_backtick(chars: &[char], index: usize) -> bool {
    let triple_at = |start: usize| {
        chars.get(start) == Some(&'`')
            && chars.get(start + 1) == Some(&'`')
            && chars.get(start + 2) == Some(&'`')
    };
    (index >= 2 && triple_at(index - 2)) || (index >= 1 && triple_at(index - 1)) || triple_at(index)
}

/// Single backticks that are neither part of a ```` ``` ```` nor escaped
/// (streamdown's `countSingleBackticks`).
fn count_single_backticks(chars: &[char]) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'`') {
            index += 2;
            continue;
        }
        if chars[index] == '`' && !is_part_of_triple_backtick(chars, index) {
            count += 1;
        }
        index += 1;
    }
    count
}

/// Count of ```` ``` ```` runs anywhere (streamdown's `(text.match(/```/g))`
/// — note `\`\`\`\`` counts twice, matching JS `String.match`).
fn count_triple_backtick_runs(chars: &[char]) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index + 2 < chars.len() + 1 {
        if chars.get(index) == Some(&'`')
            && chars.get(index + 1) == Some(&'`')
            && chars.get(index + 2) == Some(&'`')
        {
            count += 1;
            index += 3;
            continue;
        }
        index += 1;
    }
    count
}

/// `$$` pairs outside *inline* code (streamdown's `countDollarPairs`).
fn count_dollar_pairs(chars: &[char]) -> usize {
    let mut pairs = 0;
    let mut in_inline = false;
    let mut index = 0;
    while index + 1 < chars.len() {
        if chars[index] == '`' && !is_part_of_triple_backtick(chars, index) {
            in_inline = !in_inline;
        }
        if !in_inline && chars[index] == '$' && chars[index + 1] == '$' {
            pairs += 1;
            index += 2;
            continue;
        }
        index += 1;
    }
    pairs
}

/// Single `$` (excluding `$$`) outside inline code, `\$` skipped
/// (streamdown's `countSingleDollars`).
fn count_single_dollars(chars: &[char]) -> usize {
    let mut count = 0;
    let mut in_inline = false;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' {
            index += 2;
            continue;
        }
        if chars[index] == '`' && !is_part_of_triple_backtick(chars, index) {
            in_inline = !in_inline;
            index += 1;
            continue;
        }
        if !in_inline && chars[index] == '$' {
            if chars.get(index + 1) == Some(&'$') {
                index += 2;
                continue;
            }
            count += 1;
        }
        index += 1;
    }
    count
}

// ===========================================================================
// Public predicate API — the four scanners streamdown re-exports for
// authors of a custom `RemendHandler` (which receives a `&str`). Thin,
// total `&str` + char-index wrappers over the internal `&[char]` scanners.
// An out-of-range index is clamped, so these never panic.
// ===========================================================================

/// Is `character` a markdown *word character* (a Unicode letter or
/// number, or `_`) — the predicate the emphasis flanking rules use to
/// decide whether a `*`/`_`/`~` is word-internal?
///
/// streamdown's exported `isWordChar`. Useful when writing a custom
/// [`RemendHandler`] that mirrors the built-in flanking immunities.
///
/// ```
/// use rstui_ai::stream_markdown::is_word_character;
/// assert!(is_word_character('a'));
/// assert!(is_word_character('5'));
/// assert!(is_word_character('_'));
/// assert!(!is_word_character('*'));
/// assert!(!is_word_character(' '));
/// ```
#[must_use]
pub fn is_word_character(character: char) -> bool {
    is_word_char(character)
}

/// Is the char at index `char_index` of `text` inside a fenced
/// ```` ``` ```` code block (an *odd* number of triple-backtick fences
/// precede it)? streamdown's exported `isWithinCodeBlock`.
///
/// A handler should consult this before repairing a marker so it does not
/// rewrite code. An index past the end is treated as end-of-text.
///
/// ```
/// use rstui_ai::stream_markdown::is_within_fenced_code_block;
/// let text = "```\ncode\n```";
/// assert!(is_within_fenced_code_block(text, 5)); // inside the block
/// assert!(!is_within_fenced_code_block("before ```c``` after", 2));
/// ```
#[must_use]
pub fn is_within_fenced_code_block(text: &str, char_index: usize) -> bool {
    let chars: Vec<char> = text.chars().collect();
    within_fenced_block(&chars, char_index.min(chars.len()))
}

/// Is the char at index `char_index` of `text` inside a `[…](url)` link
/// or image URL? streamdown's exported `isWithinLinkOrImageUrl`.
///
/// ```
/// use rstui_ai::stream_markdown::is_within_link_or_image_url;
/// assert!(is_within_link_or_image_url("[t](http://example.com)", 10));
/// assert!(!is_within_link_or_image_url("before [t](u) after", 2));
/// ```
#[must_use]
pub fn is_within_link_or_image_url(text: &str, char_index: usize) -> bool {
    let chars: Vec<char> = text.chars().collect();
    within_link_or_image_url(&chars, char_index.min(chars.len()))
}

/// Is the char at index `char_index` of `text` inside `$…$` / `$$…$$`
/// math (block math takes precedence; `\$` is skipped)? streamdown's
/// exported `isWithinMathBlock`.
///
/// ```
/// use rstui_ai::stream_markdown::is_within_math_block;
/// assert!(is_within_math_block("$$x^2$$", 3));
/// assert!(!is_within_math_block("before $x$ after", 14));
/// ```
#[must_use]
pub fn is_within_math_block(text: &str, char_index: usize) -> bool {
    let chars: Vec<char> = text.chars().collect();
    within_math_block(&chars, char_index.min(chars.len()))
}

// ===========================================================================
// The handlers (streamdown's handler files), each `&[char]` → Option<String>
// where `None` means "left unchanged". Names match the streamdown handlers.
// ===========================================================================

/// streamdown's regex `(\*\*)([^*]*\*?)$`: trailing `**` then non-`*`
/// content (optionally one closing `*`). Returns the content tail (group 2)
/// as a char slice if it matches.
fn match_trailing_double_asterisk(chars: &[char]) -> Option<(usize, Vec<char>)> {
    // Find the last `**`, then require everything after it to be `[^*]*\*?`.
    let pair_start = last_index_of(chars, &['*', '*'])?;
    let tail = &chars[pair_start + 2..];
    let (body, trailing_star) = match tail.split_last() {
        Some((&'*', rest)) => (rest, true),
        _ => (tail, false),
    };
    if body.contains(&'*') {
        return None;
    }
    let mut group2 = body.to_vec();
    if trailing_star {
        group2.push('*');
    }
    Some((pair_start, group2))
}

/// streamdown's regex `(__)([^_]*?)$`: trailing `__` then non-`_` content.
fn match_trailing_double_underscore(chars: &[char]) -> Option<(usize, Vec<char>)> {
    let pair_start = last_index_of(chars, &['_', '_'])?;
    let tail = &chars[pair_start + 2..];
    if tail.contains(&'_') {
        return None;
    }
    Some((pair_start, tail.to_vec()))
}

/// streamdown's regex `(__)([^_]+)_$`: `__`, `>=1` non-`_`, then a lone
/// closing `_` (half-typed close).
fn match_half_complete_underscore(chars: &[char]) -> Option<usize> {
    if chars.last() != Some(&'_') {
        return None;
    }
    let body = &chars[..chars.len() - 1];
    let pair_start = last_index_of(body, &['_', '_'])?;
    let between = &body[pair_start + 2..];
    if between.is_empty() || between.contains(&'_') {
        return None;
    }
    Some(pair_start)
}

/// streamdown's regex `(~~)([^~]+)~$`.
fn match_half_complete_tilde(chars: &[char]) -> Option<usize> {
    if chars.last() != Some(&'~') {
        return None;
    }
    let body = &chars[..chars.len() - 1];
    let pair_start = last_index_of(body, &['~', '~'])?;
    let between = &body[pair_start + 2..];
    if between.is_empty() || between.contains(&'~') {
        return None;
    }
    Some(pair_start)
}

/// streamdown's regex `(\*\*\*)([^*]*?)$`.
fn match_trailing_triple_asterisk(chars: &[char]) -> Option<(usize, Vec<char>)> {
    let triple_start = last_index_of(chars, &['*', '*', '*'])?;
    let tail = &chars[triple_start + 3..];
    if tail.contains(&'*') {
        return None;
    }
    Some((triple_start, tail.to_vec()))
}

/// streamdown's regex `(\*)([^*]*?)$` matched (the *content*, group 2, is
/// what the caller inspects; the marker is located separately).
fn matches_trailing_single_asterisk(chars: &[char]) -> bool {
    // `(\*)([^*]*?)$`: there is a `*` such that everything after it is
    // non-`*`. The last `*` in the string always satisfies this.
    chars.contains(&'*')
}

/// streamdown's regex `(_)([^_]*?)$`.
fn matches_trailing_single_underscore(chars: &[char]) -> bool {
    chars.contains(&'_')
}

/// streamdown's regex `(`)([^`]*?)$`: a `` ` `` then non-`` ` `` content.
/// Returns the content tail if matched.
fn match_trailing_single_backtick(chars: &[char]) -> Option<Vec<char>> {
    let tick = chars.iter().rposition(|&c| c == '`')?;
    Some(chars[tick + 1..].to_vec())
}

/// streamdown's regex `(~~)([^~]*?)$`.
fn match_trailing_double_tilde(chars: &[char]) -> Option<(usize, Vec<char>)> {
    let pair_start = last_index_of(chars, &['~', '~'])?;
    let tail = &chars[pair_start + 2..];
    if tail.contains(&'~') {
        return None;
    }
    Some((pair_start, tail.to_vec()))
}

/// Line containing `marker_index`, from the char after the previous `\n`
/// up to (not including) `marker_index` — streamdown's `lineBeforeMarker`.
fn line_before(chars: &[char], marker_index: usize) -> &[char] {
    let mut start = 0;
    let mut scan = marker_index as isize - 1;
    while scan >= 0 {
        if chars[scan as usize] == '\n' {
            start = scan as usize + 1;
            break;
        }
        scan -= 1;
    }
    &chars[start..marker_index]
}

/// `handleSingleTildeEscape` (priority 0): escape a single `~` flanked by
/// word chars (and not in a code block) → `\~`.
fn handle_single_tilde_escape(chars: &[char]) -> Option<String> {
    if !chars.contains(&'~') {
        return None;
    }
    let mut out = String::with_capacity(chars.len() + 4);
    let mut changed = false;
    for (index, &character) in chars.iter().enumerate() {
        if character == '~' {
            let prev = if index > 0 { chars[index - 1] } else { '\0' };
            let next = chars.get(index + 1).copied().unwrap_or('\0');
            let flanked = is_word_char(prev) && is_word_char(next) && prev != '~' && next != '~';
            if flanked && !within_code_span_or_fence(chars, index) {
                out.push('\\');
                out.push('~');
                changed = true;
                continue;
            }
        }
        out.push(character);
    }
    changed.then_some(out)
}

/// `handleComparisonOperators` (priority 5): in a list item, `>` (then
/// optional `=`, optional `$`, a digit) is a comparison, not a quote →
/// escape the `>`. Pattern `^(\s*(?:[-*+]|\d+[.)]) +)>(=?\s*[$]?\d)`, `gm`.
fn handle_comparison_operators(chars: &[char]) -> Option<String> {
    if !chars.contains(&'>') {
        return None;
    }
    let mut out = String::with_capacity(chars.len() + 4);
    let mut changed = false;
    let mut index = 0;
    let mut at_line_start = true;
    while index < chars.len() {
        if at_line_start {
            if let Some(consumed) = try_escape_list_comparison(chars, index, &mut out) {
                changed = true;
                index += consumed;
                at_line_start = chars.get(index - 1) == Some(&'\n');
                continue;
            }
        }
        let character = chars[index];
        out.push(character);
        at_line_start = character == '\n';
        index += 1;
    }
    changed.then_some(out)
}

/// At `start` (a line start), try to match the list-comparison pattern and
/// emit the escaped form into `out`. Returns chars consumed, or `None`.
fn try_escape_list_comparison(chars: &[char], start: usize, out: &mut String) -> Option<usize> {
    let mut index = start;
    // `\s*` (but not a newline — `^` is per-line under `gm`).
    while index < chars.len() && (chars[index] == ' ' || chars[index] == '\t') {
        index += 1;
    }
    // `(?:[-*+]|\d+[.)])`
    let marker_ok = if index < chars.len() && matches!(chars[index], '-' | '*' | '+') {
        index += 1;
        true
    } else {
        let digits_start = index;
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
        if index > digits_start && index < chars.len() && matches!(chars[index], '.' | ')') {
            index += 1;
            true
        } else {
            false
        }
    };
    if !marker_ok {
        return None;
    }
    // ` +` (one or more spaces)
    let spaces_start = index;
    while index < chars.len() && chars[index] == ' ' {
        index += 1;
    }
    if index == spaces_start {
        return None;
    }
    // `>`
    if chars.get(index) != Some(&'>') {
        return None;
    }
    let gt_index = index;
    index += 1;
    // `(=?\s*[$]?\d)`
    let suffix_start = index;
    if chars.get(index) == Some(&'=') {
        index += 1;
    }
    while index < chars.len() && (chars[index] == ' ' || chars[index] == '\t') {
        index += 1;
    }
    if chars.get(index) == Some(&'$') {
        index += 1;
    }
    if index >= chars.len() || !chars[index].is_ascii_digit() {
        return None;
    }
    index += 1;
    if within_code_span_or_fence(chars, start) {
        return None;
    }
    // Emit: prefix `\>` suffix.
    for &character in &chars[start..gt_index] {
        out.push(character);
    }
    out.push('\\');
    out.push('>');
    for &character in &chars[suffix_start..index] {
        out.push(character);
    }
    Some(index - start)
}

/// `handleIncompleteHtmlTag` (priority 10): strip a trailing `<tag…` (no
/// `>`), and the whitespace before it. Pattern `<[a-zA-Z/][^>]*$`.
fn handle_incomplete_html_tag(chars: &[char]) -> Option<String> {
    // Find the last `<` that begins `<[a-zA-Z/]` with no `>` after it.
    let mut scan = chars.len() as isize - 1;
    let mut tag_start: Option<usize> = None;
    while scan >= 0 {
        let here = scan as usize;
        if chars[here] == '>' {
            return None;
        }
        if chars[here] == '<' {
            let next = chars.get(here + 1).copied().unwrap_or('\0');
            if next.is_ascii_alphabetic() || next == '/' {
                tag_start = Some(here);
            }
            break;
        }
        scan -= 1;
    }
    let start = tag_start?;
    if within_code_span_or_fence(chars, start) {
        return None;
    }
    let mut end = start;
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    Some(chars[..end].iter().collect())
}

/// `handleIncompleteSetextHeading` (priority 15): a last line of only
/// `-`/`--`/`=`/`==` (no trailing space) after a non-empty previous line
/// would parse as a setext underline → append `\u{200b}` to break it.
fn handle_incomplete_setext_heading(chars: &[char]) -> Option<String> {
    let newline = chars.iter().rposition(|&c| c == '\n')?;
    let last_line = &chars[newline + 1..];
    let previous = &chars[..newline];

    let trimmed: Vec<char> = {
        let mut start = 0;
        let mut end = last_line.len();
        while start < end && last_line[start].is_whitespace() {
            start += 1;
        }
        while end > start && last_line[end - 1].is_whitespace() {
            end -= 1;
        }
        last_line[start..end].to_vec()
    };

    let only = |marker: char| -> bool {
        (trimmed.len() == 1 || trimmed.len() == 2) && trimmed.iter().all(|&c| c == marker)
    };
    // `^[\s]*<m>{1,2}[\s]+$` — already space-broken, leave it.
    let space_broken = |marker: char| -> bool {
        let mut index = 0;
        while index < last_line.len() && last_line[index].is_whitespace() {
            index += 1;
        }
        let run_start = index;
        while index < last_line.len() && last_line[index] == marker {
            index += 1;
        }
        let run = index - run_start;
        (run == 1 || run == 2)
            && index < last_line.len()
            && last_line[index..].iter().all(|c| c.is_whitespace())
            && last_line.len() > index
    };

    let previous_line_has_content = {
        let start = previous
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |n| n + 1);
        previous[start..].iter().any(|c| !c.is_whitespace())
    };

    for marker in ['-', '='] {
        if only(marker) && !space_broken(marker) && previous_line_has_content {
            let mut out: String = chars.iter().collect();
            out.push('\u{200b}');
            return Some(out);
        }
    }
    None
}

/// `handleIncompleteLinksAndImages` (priority 20). May early-return the
/// whole pipeline (signalled by the caller checking the placeholder
/// suffix). Returns the (possibly unchanged-shaped) repaired string, or
/// `None` for "no link/image structure to act on".
fn handle_incomplete_links_and_images(chars: &[char], mode: LinkMode) -> Option<String> {
    // 1. Trailing `](` — an incomplete URL.
    if let Some(paren) = last_index_of(chars, &[']', '(']) {
        if !within_code_span_or_fence(chars, paren) {
            if let Some(result) = incomplete_url(chars, paren, mode) {
                return Some(result);
            }
        }
    }
    // 2. Backwards search for an unclosed `[`.
    let mut index = chars.len() as isize - 1;
    while index >= 0 {
        let here = index as usize;
        if chars[here] == '[' && !within_code_span_or_fence(chars, here) {
            if let Some(result) = incomplete_link_text(chars, here, mode) {
                return Some(result);
            }
        }
        index -= 1;
    }
    None
}

/// streamdown's `handleIncompleteUrl`.
fn incomplete_url(chars: &[char], paren_index: usize, mode: LinkMode) -> Option<String> {
    let after: Vec<char> = chars.get(paren_index + 2..).unwrap_or(&[]).to_vec();
    if after.contains(&')') {
        return None;
    }
    let open_bracket = matching_opening_bracket(chars, paren_index)?;
    if within_code_span_or_fence(chars, open_bracket) {
        return None;
    }
    let is_image = open_bracket > 0 && chars[open_bracket - 1] == '!';
    let start = if is_image {
        open_bracket - 1
    } else {
        open_bracket
    };
    let before: String = chars[..start].iter().collect();
    if is_image {
        return Some(before);
    }
    let link_text: String = chars[open_bracket + 1..paren_index].iter().collect();
    match mode {
        LinkMode::TextOnly => Some(format!("{before}{link_text}")),
        LinkMode::Protocol => Some(format!("{before}[{link_text}]({INCOMPLETE_LINK_URL})")),
    }
}

/// streamdown's `findFirstIncompleteBracket` (text-only mode).
fn first_incomplete_bracket(chars: &[char], max_pos: usize) -> usize {
    let mut scan = 0;
    while scan < max_pos {
        if chars[scan] == '[' && !within_code_span_or_fence(chars, scan) {
            if scan > 0 && chars[scan - 1] == '!' {
                scan += 1;
                continue;
            }
            match matching_closing_bracket(chars, scan) {
                None => return scan,
                Some(close) => {
                    if chars.get(close + 1) == Some(&'(') {
                        if let Some(rel) = chars[close + 2..].iter().position(|&c| c == ')') {
                            scan = close + 2 + rel;
                        }
                    }
                }
            }
        }
        scan += 1;
    }
    max_pos
}

/// streamdown's `handleIncompleteText`.
fn incomplete_link_text(chars: &[char], open: usize, mode: LinkMode) -> Option<String> {
    let is_image = open > 0 && chars[open - 1] == '!';
    let open_index = if is_image { open - 1 } else { open };
    let after_open = &chars[open + 1..];

    let strip_first_incomplete = |limit: usize| -> String {
        let first = first_incomplete_bracket(chars, limit);
        let mut out: String = chars[..first].iter().collect();
        out.extend(chars[first + 1..].iter());
        out
    };

    if !after_open.contains(&']') {
        let before: String = chars[..open_index].iter().collect();
        if is_image {
            return Some(before);
        }
        return Some(match mode {
            LinkMode::TextOnly => strip_first_incomplete(open),
            LinkMode::Protocol => {
                let mut out: String = chars.iter().collect();
                out.push_str(&format!("]({INCOMPLETE_LINK_URL})"));
                out
            }
        });
    }

    match matching_closing_bracket(chars, open) {
        Some(_) => None,
        None => {
            let before: String = chars[..open_index].iter().collect();
            if is_image {
                return Some(before);
            }
            Some(match mode {
                LinkMode::TextOnly => strip_first_incomplete(open),
                LinkMode::Protocol => {
                    let mut out: String = chars.iter().collect();
                    out.push_str(&format!("]({INCOMPLETE_LINK_URL})"));
                    out
                }
            })
        }
    }
}

/// `handleIncompleteBoldItalic` (priority 30).
fn handle_incomplete_bold_italic(chars: &[char]) -> Option<String> {
    if is_four_or_more_asterisks_only(chars) {
        return None;
    }
    let (triple_start, content) = match_trailing_triple_asterisk(chars)?;
    let marker_index = last_index_of(chars, &['*', '*', '*']).unwrap_or(triple_start);

    if content.is_empty() || is_only_whitespace_or_markers(&content) {
        return None;
    }
    if within_code_span_or_fence(chars, marker_index)
        || within_complete_inline_code(chars, marker_index)
    {
        return None;
    }
    if is_horizontal_rule(chars, marker_index, '*') {
        return None;
    }
    if count_triple_asterisks(chars) % 2 == 1 {
        // Overlapping markers (`**bold and *italic***`): if `**` and `*`
        // are both balanced the `***` is a close, not an open.
        if double_asterisk_pairs(chars) % 2 == 0 && count_single_asterisks(chars) % 2 == 0 {
            return None;
        }
        let mut out: String = chars.iter().collect();
        out.push_str("***");
        return Some(out);
    }
    None
}

/// `handleIncompleteBold` (priority 35).
fn handle_incomplete_bold(chars: &[char]) -> Option<String> {
    let (pair_start, content) = match_trailing_double_asterisk(chars)?;
    let marker_index = last_index_of(chars, &['*', '*']).unwrap_or(pair_start);

    if within_code_span_or_fence(chars, marker_index)
        || within_complete_inline_code(chars, marker_index)
    {
        return None;
    }
    if content.is_empty() || is_only_whitespace_or_markers(&content) {
        return None;
    }
    // List-item-with-multiline-content immunity.
    if is_bare_list_marker_line(line_before(chars, marker_index)) && content.contains(&'\n') {
        return None;
    }
    if is_horizontal_rule(chars, marker_index, '*') {
        return None;
    }
    if double_asterisk_pairs(chars) % 2 == 1 {
        let mut out: String = chars.iter().collect();
        if content.last() == Some(&'*') {
            out.push('*');
        } else {
            out.push_str("**");
        }
        return Some(out);
    }
    None
}

/// `handleIncompleteDoubleUnderscoreItalic` (priority 40).
fn handle_incomplete_double_underscore_italic(chars: &[char]) -> Option<String> {
    match match_trailing_double_underscore(chars) {
        None => {
            // Half-complete `__x_` → `__x__`.
            let pair_start = match_half_complete_underscore(chars)?;
            if within_code_span_or_fence(chars, pair_start)
                || within_complete_inline_code(chars, pair_start)
            {
                return None;
            }
            if double_underscore_pairs(chars) % 2 == 1 {
                let mut out: String = chars.iter().collect();
                out.push('_');
                return Some(out);
            }
            None
        }
        Some((pair_start, content)) => {
            let marker_index = last_index_of(chars, &['_', '_']).unwrap_or(pair_start);
            if within_code_span_or_fence(chars, marker_index)
                || within_complete_inline_code(chars, marker_index)
            {
                return None;
            }
            if content.is_empty() || is_only_whitespace_or_markers(&content) {
                return None;
            }
            if is_bare_list_marker_line(line_before(chars, marker_index)) && content.contains(&'\n')
            {
                return None;
            }
            if is_horizontal_rule(chars, marker_index, '_') {
                return None;
            }
            if double_underscore_pairs(chars) % 2 == 1 {
                let mut out: String = chars.iter().collect();
                out.push_str("__");
                return Some(out);
            }
            None
        }
    }
}

/// First single-asterisk index, skipping fenced blocks / math /
/// whitespace-flanked / word-internal (streamdown's
/// `findFirstSingleAsteriskIndex`).
fn first_single_asterisk_index(chars: &[char]) -> Option<usize> {
    let mut in_fence = false;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`'
            && index + 2 < chars.len()
            && chars[index + 1] == '`'
            && chars[index + 2] == '`'
        {
            in_fence = !in_fence;
            index += 3;
            continue;
        }
        if in_fence {
            index += 1;
            continue;
        }
        let prev = if index > 0 { chars[index - 1] } else { '\0' };
        let next = chars.get(index + 1).copied().unwrap_or('\0');
        if chars[index] == '*'
            && prev != '*'
            && next != '*'
            && prev != '\\'
            && !within_math_block(chars, index)
        {
            let prev_ws = prev == '\0' || prev == ' ' || prev == '\t' || prev == '\n';
            let next_ws = next == '\0' || next == ' ' || next == '\t' || next == '\n';
            if prev_ws && next_ws {
                index += 1;
                continue;
            }
            if prev != '\0' && next != '\0' && is_word_char(prev) && is_word_char(next) {
                index += 1;
                continue;
            }
            return Some(index);
        }
        index += 1;
    }
    None
}

/// `handleIncompleteSingleAsteriskItalic` (priority 41).
fn handle_incomplete_single_asterisk_italic(chars: &[char]) -> Option<String> {
    if !matches_trailing_single_asterisk(chars) {
        return None;
    }
    let first = first_single_asterisk_index(chars)?;
    if within_code_span_or_fence(chars, first) || within_complete_inline_code(chars, first) {
        return None;
    }
    let content = &chars[first + 1..];
    if content.is_empty() || is_only_whitespace_or_markers(content) {
        return None;
    }
    if count_single_asterisks(chars) % 2 == 1 {
        let mut out: String = chars.iter().collect();
        out.push('*');
        return Some(out);
    }
    None
}

/// First single-underscore index (streamdown's
/// `findFirstSingleUnderscoreIndex`).
fn first_single_underscore_index(chars: &[char]) -> Option<usize> {
    let mut in_fence = false;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`'
            && index + 2 < chars.len()
            && chars[index + 1] == '`'
            && chars[index + 2] == '`'
        {
            in_fence = !in_fence;
            index += 3;
            continue;
        }
        if in_fence {
            index += 1;
            continue;
        }
        let prev = if index > 0 { chars[index - 1] } else { '\0' };
        let next = chars.get(index + 1).copied().unwrap_or('\0');
        if chars[index] == '_'
            && prev != '_'
            && next != '_'
            && prev != '\\'
            && !within_math_block(chars, index)
            && !within_link_or_image_url(chars, index)
        {
            if prev != '\0' && next != '\0' && is_word_char(prev) && is_word_char(next) {
                index += 1;
                continue;
            }
            return Some(index);
        }
        index += 1;
    }
    None
}

/// streamdown's `insertClosingUnderscore` (place `_` before trailing
/// newlines).
fn insert_closing_underscore(chars: &[char]) -> String {
    let mut end = chars.len();
    while end > 0 && chars[end - 1] == '\n' {
        end -= 1;
    }
    if end < chars.len() {
        let mut out: String = chars[..end].iter().collect();
        out.push('_');
        out.extend(chars[end..].iter());
        out
    } else {
        let mut out: String = chars.iter().collect();
        out.push('_');
        out
    }
}

/// streamdown's `handleTrailingAsterisksForUnderscore` (nest `_` inside a
/// just-closed `**`).
fn trailing_asterisks_for_underscore(chars: &[char]) -> Option<String> {
    if !(chars.len() >= 2 && chars[chars.len() - 2..] == ['*', '*']) {
        return None;
    }
    let without = &chars[..chars.len() - 2];
    if double_asterisk_pairs(without) % 2 != 1 {
        return None;
    }
    let first_double = {
        let mut found = None;
        let mut index = 0;
        while index + 1 < without.len() {
            if without[index] == '*' && without[index + 1] == '*' {
                found = Some(index);
                break;
            }
            index += 1;
        }
        found
    };
    let underscore = first_single_underscore_index(without);
    if let (Some(double_index), Some(underscore_index)) = (first_double, underscore) {
        if double_index < underscore_index {
            let mut out: String = without.iter().collect();
            out.push('_');
            out.push_str("**");
            return Some(out);
        }
    }
    None
}

/// `handleIncompleteSingleUnderscoreItalic` (priority 42).
fn handle_incomplete_single_underscore_italic(chars: &[char]) -> Option<String> {
    if !matches_trailing_single_underscore(chars) {
        return None;
    }
    let first = first_single_underscore_index(chars)?;
    let content = &chars[first + 1..];
    if content.is_empty() || is_only_whitespace_or_markers(content) {
        return None;
    }
    if within_code_span_or_fence(chars, first) || within_complete_inline_code(chars, first) {
        return None;
    }
    if count_single_underscores(chars) % 2 == 1 {
        if let Some(nested) = trailing_asterisks_for_underscore(chars) {
            return Some(nested);
        }
        return Some(insert_closing_underscore(chars));
    }
    None
}

/// `handleIncompleteInlineCode` (priority 50).
fn handle_incomplete_inline_code(chars: &[char]) -> Option<String> {
    // Inline triple backticks: `^```[^`\n]*```?$` with no newline.
    if !chars.contains(&'\n') && is_inline_triple_backtick_shape(chars) {
        let ends_double = chars.len() >= 2 && chars[chars.len() - 2..] == ['`', '`'];
        let ends_triple = chars.len() >= 3 && chars[chars.len() - 3..] == ['`', '`', '`'];
        if ends_double && !ends_triple {
            let mut out: String = chars.iter().collect();
            out.push('`');
            return Some(out);
        }
        return None;
    }

    let content = match_trailing_single_backtick(chars)?;
    // Inside an incomplete fenced block? (odd count of ```` ``` ````.)
    if count_triple_backtick_runs(chars) % 2 == 1 {
        return None;
    }
    if content.is_empty() || is_only_whitespace_or_markers(&content) {
        return None;
    }
    if count_single_backticks(chars) % 2 == 1 {
        let mut out: String = chars.iter().collect();
        out.push('`');
        return Some(out);
    }
    None
}

/// `^```[^`\n]*```?$` (streamdown's `inlineTripleBacktickPattern`).
fn is_inline_triple_backtick_shape(chars: &[char]) -> bool {
    if chars.len() < 4 || chars[..3] != ['`', '`', '`'] {
        return false;
    }
    // `[^`\n]*` then `` ``? `` (2 or 3 trailing backticks) at the very end.
    let body_end = if chars.len() >= 3 && chars[chars.len() - 3..] == ['`', '`', '`'] {
        chars.len() - 3
    } else if chars.len() >= 2 && chars[chars.len() - 2..] == ['`', '`'] {
        chars.len() - 2
    } else {
        return false;
    };
    if body_end < 3 {
        return false;
    }
    chars[3..body_end].iter().all(|&c| c != '`' && c != '\n')
}

/// `handleIncompleteStrikethrough` (priority 60).
fn handle_incomplete_strikethrough(chars: &[char]) -> Option<String> {
    match match_trailing_double_tilde(chars) {
        Some((pair_start, content)) => {
            if content.is_empty() || is_only_whitespace_or_markers(&content) {
                return None;
            }
            let marker_index = last_index_of(chars, &['~', '~']).unwrap_or(pair_start);
            if within_code_span_or_fence(chars, marker_index)
                || within_complete_inline_code(chars, marker_index)
            {
                return None;
            }
            if double_tilde_pairs(chars) % 2 == 1 {
                let mut out: String = chars.iter().collect();
                out.push_str("~~");
                return Some(out);
            }
            None
        }
        None => {
            let pair_start = match_half_complete_tilde(chars)?;
            if within_code_span_or_fence(chars, pair_start)
                || within_complete_inline_code(chars, pair_start)
            {
                return None;
            }
            if double_tilde_pairs(chars) % 2 == 1 {
                let mut out: String = chars.iter().collect();
                out.push('~');
                return Some(out);
            }
            None
        }
    }
}

/// `handleIncompleteBlockKatex` (priority 70).
fn handle_incomplete_block_katex(chars: &[char]) -> Option<String> {
    if count_dollar_pairs(chars) % 2 == 0 {
        return None;
    }
    // `addClosingKatex`.
    let ends_dollar = chars.last() == Some(&'$');
    let ends_double = chars.len() >= 2 && chars[chars.len() - 2..] == ['$', '$'];
    if ends_dollar && !ends_double {
        let mut out: String = chars.iter().collect();
        out.push('$');
        return Some(out);
    }
    let first_double = last_index_of_first(chars, &['$', '$']);
    let newline_after = first_double
        .map(|start| chars[start..].contains(&'\n'))
        .unwrap_or(false);
    let mut out: String = chars.iter().collect();
    if newline_after && chars.last() != Some(&'\n') {
        out.push('\n');
        out.push_str("$$");
    } else {
        out.push_str("$$");
    }
    Some(out)
}

/// First (not last) occurrence of `needle` — streamdown uses `indexOf`
/// for the `$$` start.
fn last_index_of_first(chars: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > chars.len() {
        return None;
    }
    let mut start = 0;
    while start + needle.len() <= chars.len() {
        if chars[start..start + needle.len()] == *needle {
            return Some(start);
        }
        start += 1;
    }
    None
}

/// `handleIncompleteInlineKatex` (priority 75, opt-in).
fn handle_incomplete_inline_katex(chars: &[char]) -> Option<String> {
    if count_single_dollars(chars) % 2 == 1 {
        let mut out: String = chars.iter().collect();
        out.push('$');
        return Some(out);
    }
    None
}

// ===========================================================================
// The driver.
// ===========================================================================

/// Repair an in-progress markdown string so a complete-document renderer
/// shows it the way the *finished* document will render — the streamdown
/// `remend` pipeline.
///
/// The driver, faithful to streamdown:
///
/// 1. If `text` ends with exactly one space (not two — a trailing
///    **double** space is a hard line break and is preserved), strip that
///    one space.
/// 2. Run every enabled built-in handler in ascending priority
///    (single-tilde 0, comparison 5, html 10, setext 15, links/images 20,
///    bold-italic 30, bold 35, italic `__`/`*`/`_` 40/41/42, inline-code
///    50, strikethrough 60, block-KaTeX 70, inline-KaTeX 75) then the
///    caller's [`handlers`](RemendOptions::handlers) (default priority
///    [`DEFAULT_HANDLER_PRIORITY`]), a stable sort so equal priorities
///    keep insertion order.
/// 3. The links/images handler *early-returns* the whole pipeline the
///    moment it has produced the `streamdown:incomplete-link` placeholder
///    (protocol mode only) — exactly streamdown's behaviour, so a closed
///    placeholder link is never then mangled by the emphasis handlers.
///
/// Empty input is returned unchanged. The function is total (no input
/// panics) and idempotent (`remend(remend(x), o) == remend(x, o)`).
///
/// # Example
///
/// ```
/// use rstui_ai::stream_markdown::{remend, RemendOptions};
///
/// let options = RemendOptions::default();
/// // An open bold run is closed so no literal `**` is shown.
/// assert_eq!(remend("This is **bol", &options), "This is **bol**");
/// // An open link becomes the placeholder (protocol mode).
/// assert_eq!(
///     remend("see [the docs](http", &options),
///     "see [the docs](streamdown:incomplete-link)"
/// );
/// // A finished document is returned untouched, and the repair is
/// // idempotent.
/// let done = "# Title\n\nAll **done**.";
/// assert_eq!(remend(done, &options), done);
/// assert_eq!(remend(&remend(done, &options), &options), remend(done, &options));
/// ```
#[must_use]
pub fn remend(text: &str, options: &RemendOptions) -> String {
    if text.is_empty() {
        return String::new();
    }

    // 1. Trailing single-space strip (double space = hard break, kept).
    let trimmed: Cow<'_, str> = if text.ends_with(' ') && !text.ends_with("  ") {
        Cow::Owned(text[..text.len() - 1].to_string())
    } else {
        Cow::Borrowed(text)
    };

    // The ordered, enabled built-in pipeline. Each entry: (priority,
    // transform, early_return_on_placeholder).
    type Pass<'a> = (i32, Box<dyn Fn(&[char]) -> Option<String> + 'a>, bool);
    let mut passes: Vec<Pass<'_>> = Vec::new();

    if options.single_tilde {
        passes.push((0, Box::new(handle_single_tilde_escape), false));
    }
    if options.comparison_operators {
        passes.push((5, Box::new(handle_comparison_operators), false));
    }
    if options.html_tags {
        passes.push((10, Box::new(handle_incomplete_html_tag), false));
    }
    if options.setext_headings {
        passes.push((15, Box::new(handle_incomplete_setext_heading), false));
    }
    if options.links || options.images {
        let mode = options.link_mode;
        let early = mode == LinkMode::Protocol;
        passes.push((
            20,
            Box::new(move |chars: &[char]| handle_incomplete_links_and_images(chars, mode)),
            early,
        ));
    }
    if options.bold_italic {
        passes.push((30, Box::new(handle_incomplete_bold_italic), false));
    }
    if options.bold {
        passes.push((35, Box::new(handle_incomplete_bold), false));
    }
    if options.italic {
        passes.push((
            40,
            Box::new(handle_incomplete_double_underscore_italic),
            false,
        ));
        passes.push((
            41,
            Box::new(handle_incomplete_single_asterisk_italic),
            false,
        ));
        passes.push((
            42,
            Box::new(handle_incomplete_single_underscore_italic),
            false,
        ));
    }
    if options.inline_code {
        passes.push((50, Box::new(handle_incomplete_inline_code), false));
    }
    if options.strikethrough {
        passes.push((60, Box::new(handle_incomplete_strikethrough), false));
    }
    if options.katex {
        passes.push((70, Box::new(handle_incomplete_block_katex), false));
    }
    if options.inline_katex {
        passes.push((75, Box::new(handle_incomplete_inline_katex), false));
    }

    let custom_base = passes.len();
    for handler in &options.handlers {
        let transform = handler.transform;
        passes.push((
            handler.priority,
            Box::new(move |chars: &[char]| Some(transform(&chars.iter().collect::<String>()))),
            false,
        ));
    }

    // Stable sort by priority (equal priority keeps insertion order:
    // built-ins before customs, customs in given order).
    let mut order: Vec<usize> = (0..passes.len()).collect();
    order.sort_by_key(|&i| {
        let secondary = if i >= custom_base { i } else { 0 };
        (passes[i].0, secondary, i)
    });

    let mut current: String = trimmed.into_owned();
    for &pass_index in &order {
        let (_, transform, early_return) = &passes[pass_index];
        let chars: Vec<char> = current.chars().collect();
        if let Some(next) = transform(&chars) {
            current = next;
        }
        if *early_return && current.ends_with(&format!("]({INCOMPLETE_LINK_URL})")) {
            return current;
        }
    }
    current
}

// ===========================================================================
// Block segmentation + per-block render cache.
// ===========================================================================

/// `^\s{0,3}(`{3,}|~{3,})` — an opening fence line (≤3 lead spaces).
fn fence_marker(line: &str) -> Option<char> {
    let mut chars = line.chars();
    let mut spaces = 0;
    let mut peeked = chars.clone();
    while let Some(' ') = peeked.next() {
        spaces += 1;
        if spaces > 3 {
            return None;
        }
        chars.next();
    }
    let rest: Vec<char> = chars.collect();
    if rest.len() >= 3 && rest[..3].iter().all(|&c| c == '`') {
        Some('`')
    } else if rest.len() >= 3 && rest[..3].iter().all(|&c| c == '~') {
        Some('~')
    } else {
        None
    }
}

/// Is `line` a closing fence for an open `marker` fence (a run of ≥3 of
/// the same marker, ≤3 lead spaces, nothing else of substance)?
fn is_closing_fence(line: &str, marker: char) -> bool {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    let run = trimmed.chars().take_while(|&c| c == marker).count();
    run >= 3 && trimmed[run..].trim().is_empty()
}

/// Does the document end with an *open* fenced code block — an opening
/// fence line with no matching close after it?
///
/// Used by [`StreamMarkdown`] to render the dangling block as plain
/// monospace text (defer the highlighter, which would otherwise re-style
/// on every token) and to suppress a trailing caret.
#[must_use]
pub fn last_block_has_open_fence(text: &str) -> bool {
    let mut open: Option<char> = None;
    for line in text.split('\n') {
        match open {
            None => {
                if let Some(marker) = fence_marker(line) {
                    open = Some(marker);
                }
            }
            Some(marker) => {
                if is_closing_fence(line, marker) {
                    open = None;
                }
            }
        }
    }
    open.is_some()
}

/// Split a *repaired* markdown document into top-level block source
/// strings (each is a self-contained chunk
/// [`rstui_widgets::Markdown`] can render on its own and the
/// [`StreamCache`] can key on).
///
/// Splitting rule: blank line(s) separate blocks, **except**
///
/// - an **unterminated fenced** ```` ``` ````/`~~~` block — everything
///   from the opening fence to end-of-input is one segment (a blank line
///   inside streamed code must not tear the block);
/// - an **unterminated `$$` block** — same, from the opening `$$` to
///   end-of-input;
/// - an **unclosed HTML block** — a block opening with `<tag` whose
///   closing `</tag>`/`>` has not arrived stays merged with what follows;
/// - a document that contains a **footnote** reference `[^id]` or
///   definition `[^id]:` is kept **whole** (one block), because the
///   definition and its reference must be rendered in one
///   [`rstui_widgets::Markdown`] pass to resolve.
///
/// Total: any input (empty, only blanks, a lone open fence) yields a
/// well-formed `Vec` (possibly empty), never a panic.
#[must_use]
pub fn parse_into_blocks(repaired: &str) -> Vec<String> {
    if repaired.is_empty() {
        return Vec::new();
    }

    // Footnotes must resolve across the whole doc → keep it whole.
    if contains_footnote(repaired) {
        return vec![repaired.to_string()];
    }

    let lines: Vec<&str> = repaired.split('\n').collect();
    let mut blocks: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut open_fence: Option<char> = None;
    let mut in_block_math = false;
    let mut open_html = false;

    let flush = |current: &mut Vec<&str>, blocks: &mut Vec<String>| {
        if !current.is_empty() {
            blocks.push(current.join("\n"));
            current.clear();
        }
    };

    for line in lines {
        if let Some(marker) = open_fence {
            current.push(line);
            if is_closing_fence(line, marker) {
                open_fence = None;
            }
            continue;
        }
        if in_block_math {
            current.push(line);
            if line.contains("$$") {
                in_block_math = false;
            }
            continue;
        }
        if open_html {
            current.push(line);
            if line.contains('>') {
                open_html = false;
            }
            continue;
        }

        if let Some(marker) = fence_marker(line) {
            current.push(line);
            if !is_closing_fence_after_open(line, marker) {
                open_fence = Some(marker);
            }
            continue;
        }

        let opens_block_math = opens_unbalanced_block_math(line);
        if opens_block_math {
            current.push(line);
            in_block_math = true;
            continue;
        }

        if opens_unclosed_html_block(line) {
            current.push(line);
            open_html = true;
            continue;
        }

        if line.trim().is_empty() {
            flush(&mut current, &mut blocks);
        } else {
            current.push(line);
        }
    }
    flush(&mut current, &mut blocks);
    blocks
}

/// A footnote reference (`[^id]`) or definition (`^[^id]:`) anywhere.
fn contains_footnote(text: &str) -> bool {
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == '[' && bytes[index + 1] == '^' {
            // Need a closing `]` on the same line with non-empty id.
            let mut scan = index + 2;
            let mut id_len = 0;
            while scan < bytes.len() && bytes[scan] != ']' && bytes[scan] != '\n' {
                id_len += 1;
                scan += 1;
            }
            if id_len > 0 && bytes.get(scan) == Some(&']') {
                return true;
            }
        }
        index += 1;
    }
    false
}

/// A fence line that *also* closes on the same line (e.g. an inline
/// ```` ```code``` ````) does not open a multi-line block.
fn is_closing_fence_after_open(line: &str, marker: char) -> bool {
    let trimmed = line.trim_start_matches(' ');
    let open_run = trimmed.chars().take_while(|&c| c == marker).count();
    if open_run < 3 {
        return false;
    }
    let after = &trimmed[open_run..];
    // Another run of >=3 of the same marker later on the line = closed.
    let mut run = 0;
    for character in after.chars() {
        if character == marker {
            run += 1;
            if run >= 3 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// A line that opens a `$$` block that does not also close on the line.
fn opens_unbalanced_block_math(line: &str) -> bool {
    let occurrences = line.matches("$$").count();
    occurrences % 2 == 1
}

/// A crude block-level HTML open: a line starting (after ≤3 spaces) with
/// `<tag` whose matching `>` has not appeared on the line.
fn opens_unclosed_html_block(line: &str) -> bool {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    let mut chars = trimmed.chars();
    if chars.next() != Some('<') {
        return false;
    }
    match chars.next() {
        Some(next) if next.is_ascii_alphabetic() || next == '/' => {}
        _ => return false,
    }
    !trimmed.contains('>')
}

/// A per-block render cache: the rendered [`Line`]s of each block, keyed
/// by that block's exact source string.
///
/// During streaming only the *last* block changes token-to-token; every
/// earlier block's source is byte-for-byte stable. Re-running
/// [`rstui_widgets::Markdown`] over the whole document each token would
/// re-lay-out (and re-highlight) settled prose for nothing. This cache
/// keeps `(source, lines)` per block: on [`StreamCache::ingest`] a block
/// whose source matches the cached entry at the same position reuses the
/// stored lines untouched; only a changed (or new) block is re-rendered.
///
/// It is plain caller-owned state (lives in [`StreamMarkdownState`]),
/// mutated only through [`StreamCache::ingest`] — never during render
/// (ADR 0012 §P1).
#[derive(Debug, Clone, Default)]
pub struct StreamCache {
    /// `(block source, rendered lines)`, in document order. Public for
    /// inspection/measurement; mutate only via [`StreamCache::ingest`].
    entries: Vec<(String, Vec<Line<'static>>)>,
    width: u16,
}

impl StreamCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The flattened rendered lines of every cached block, in order — what
    /// the widget paints. Empty until the first [`ingest`](Self::ingest).
    #[must_use]
    pub fn lines(&self) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        for (_, block_lines) in &self.entries {
            out.extend(block_lines.iter().cloned());
        }
        out
    }

    /// Number of cached blocks (for tests / measurement).
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.entries.len()
    }

    /// Re-segment `blocks` against the cache at `width`, rendering only
    /// blocks whose source changed (or are new); reuse the stored lines
    /// for every block whose source string is unchanged at its position.
    /// A width change invalidates the whole cache (layout is
    /// width-dependent).
    fn ingest_blocks(&mut self, blocks: &[String], width: u16, open_fence: bool) {
        if width != self.width {
            self.entries.clear();
            self.width = width;
        }
        let mut next: Vec<(String, Vec<Line<'static>>)> = Vec::with_capacity(blocks.len());
        for (position, source) in blocks.iter().enumerate() {
            let reuse = self
                .entries
                .get(position)
                .filter(|(cached_source, _)| cached_source == source)
                .map(|(_, lines)| lines.clone());
            let rendered = reuse.unwrap_or_else(|| {
                let is_last = position + 1 == blocks.len();
                render_block(source, width, is_last && open_fence)
            });
            next.push((source.clone(), rendered));
        }
        self.entries = next;
    }

    /// Repair → segment → (re-)render `source` at `width`, filling the
    /// cache. The caller-driven mutation step (call from `update`, never
    /// from render).
    pub fn ingest(&mut self, source: &str, width: u16, options: &RemendOptions, streaming: bool) {
        if width == 0 {
            self.entries.clear();
            self.width = 0;
            return;
        }
        if streaming {
            let repaired = remend(source, options);
            let open_fence = last_block_has_open_fence(&repaired);
            let blocks = parse_into_blocks(&repaired);
            self.ingest_blocks(&blocks, width, open_fence);
        } else {
            // Static fast path: no repair/segmentation, one whole-doc pass.
            let lines = Markdown::new(source.to_string()).lines(width);
            self.entries = vec![(source.to_string(), lines)];
            self.width = width;
        }
    }
}

/// Render one block's source to lines. A block whose own last line is an
/// open fence is rendered as plain monospace text (the highlighter is
/// deferred until the fence closes, so a streamed code block does not
/// re-colour every token); otherwise the full
/// [`rstui_widgets::Markdown`] (or [`rstui_widgets::Mermaid`] for a
/// closed ```` ```mermaid ```` block) pass runs.
fn render_block(source: &str, width: u16, open_fence: bool) -> Vec<Line<'static>> {
    if open_fence {
        // Plain text: keep the raw source verbatim, one Line per row,
        // clipped to width by the caller's layout (no markdown parse).
        return source
            .split('\n')
            .map(|row| Line::raw(row.to_string()))
            .collect();
    }
    if let Some(mermaid_source) = closed_mermaid_block(source) {
        if Mermaid::parse(&mermaid_source).is_ok() {
            // `Mermaid` is a draw-only widget (no `lines()`), so render it
            // into a scratch buffer `width` wide and a generously bounded
            // height, then read the cells back as styled lines and trim
            // trailing blank rows — the same scratch-buffer technique
            // `Markdown::link_regions` uses, kept deterministic and total.
            return widget_to_lines(Mermaid::new(mermaid_source), width, MERMAID_MAX_ROWS);
        }
        // Unparseable mermaid → fall through to plain markdown (which
        // shows it as a code block); never panic.
    }
    Markdown::new(source.to_string()).lines(width)
}

/// Scratch-buffer height when rasterising a closed Mermaid diagram: a
/// fixed, generous bound (trailing blank rows are trimmed off after) so a
/// hostile diagram cannot drive an unbounded allocation — totality over
/// adversarial input.
const MERMAID_MAX_ROWS: u16 = 512;

/// Render `widget` into a scratch `width`×`height` buffer and convert each
/// row to a [`Line`] (one [`Span`](rstui_core::Span) per cell, preserving
/// fg/bg/modifier), with trailing all-blank rows trimmed. The
/// deterministic, side-effect-free bridge from a draw-only widget to the
/// `Vec<Line>` the cache stores.
fn widget_to_lines<W: Widget>(widget: W, width: u16, height: u16) -> Vec<Line<'static>> {
    use rstui_core::{Position, Span, Style};

    if width == 0 || height == 0 {
        return Vec::new();
    }
    let mut scratch = Buffer::empty(Rect::new(0, 0, width, height));
    widget.render(scratch.area(), &mut scratch);

    let mut rows: Vec<Line<'static>> = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for x in 0..width {
            let Some(cell) = scratch.get(Position::new(x, y)) else {
                continue;
            };
            let style = Style::new()
                .fg(cell.fg)
                .bg(cell.bg)
                .add_modifier(cell.modifier);
            spans.push(Span::styled(cell.symbol.to_string(), style));
        }
        rows.push(Line::from(spans));
    }
    // Trim trailing rows that are entirely blank (a diagram rarely fills
    // its bounding box), but always keep at least one row.
    while rows.len() > 1
        && rows
            .last()
            .is_some_and(|line| line.spans.iter().all(|span| span.content.trim().is_empty()))
    {
        rows.pop();
    }
    rows
}

/// If `source` is exactly a *closed* ```` ```mermaid ```` fenced block,
/// return its inner diagram source (so the caller can delegate to
/// [`rstui_widgets::Mermaid`]); otherwise `None`.
fn closed_mermaid_block(source: &str) -> Option<String> {
    let mut lines = source.split('\n');
    let first = lines.next()?;
    let trimmed = first.trim_start();
    let marker = fence_marker(first)?;
    let info = trimmed.trim_start_matches(marker).trim();
    if !info.eq_ignore_ascii_case("mermaid") {
        return None;
    }
    let rest: Vec<&str> = lines.collect();
    // Need a closing fence as the last non-empty line.
    let close_pos = rest.iter().rposition(|l| is_closing_fence(l, marker))?;
    Some(rest[..close_pos].join("\n"))
}

// ===========================================================================
// The widget.
// ===========================================================================

/// Caller-owned state for a [`StreamMarkdown`]: the per-block
/// [`StreamCache`] plus a `settled` flag (set when streaming has
/// finished).
///
/// The model owns this; `view` reads it (through [`StreamMarkdown`]),
/// `update` mutates it via [`StreamMarkdownState::ingest`]. The widget
/// itself owns nothing (ADR 0012).
#[derive(Debug, Clone, Default)]
pub struct StreamMarkdownState {
    cache: StreamCache,
    settled: bool,
}

impl StreamMarkdownState {
    /// Fresh, empty state (nothing rendered, not settled).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` once [`mark_settled`](Self::mark_settled) was called — the
    /// stream finished. A view may use this to drop a typing caret.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.settled
    }

    /// Mark the stream finished (the final token arrived). Idempotent.
    pub fn mark_settled(&mut self) {
        self.settled = true;
    }

    /// The cache (for measurement / a surrounding scrollbar).
    #[must_use]
    pub fn cache(&self) -> &StreamCache {
        &self.cache
    }

    /// **The mutation step.** Repair → segment → (re-)render `source` at
    /// `width`, filling the cache so the next [`StreamMarkdown`] render is
    /// a pure read. Call this from `update` whenever the source or width
    /// changed — never from `view`/render.
    ///
    /// `streaming` mirrors [`StreamMarkdown::streaming`]: `false` takes
    /// the static fast path (no repair/segmentation, one whole-document
    /// [`rstui_widgets::Markdown`] pass) for a finished message.
    pub fn ingest(&mut self, source: &str, width: u16, options: &RemendOptions, streaming: bool) {
        self.cache.ingest(source, width, options, streaming);
    }
}

/// A streaming-markdown view — a pure projection of a caller-owned `&str`
/// source and a caller-owned [`StreamMarkdownState`].
///
/// Render path: the widget reads the lines the matching
/// [`StreamMarkdownState::ingest`] already cached and paints them; if the
/// cache is empty (no `ingest` yet, or a width mismatch) it falls back to
/// computing the lines inline so a first frame is never blank — the
/// computation is the same pure function, just not memoised. `update`
/// owns the (cheap, incremental) cache fill; render owns nothing.
///
/// # Example
///
/// ```
/// use rstui_core::{Buffer, Rect, Widget};
/// use rstui_ai::stream_markdown::{
///     RemendOptions, StreamMarkdown, StreamMarkdownState,
/// };
///
/// // A message still arriving: an open bold run.
/// let source = "# Hi\nThis is **bol";
/// let options = RemendOptions::default();
///
/// // `update` fills the caller-owned cache (the mutation step).
/// let mut state = StreamMarkdownState::new();
/// state.ingest(source, 24, &options, true);
///
/// // `view`/render is a pure read of that state.
/// let mut buf = Buffer::empty(Rect::new(0, 0, 24, 4));
/// StreamMarkdown::new(source)
///     .state(&state)
///     .options(options)
///     .width(24)
///     .render(buf.area(), &mut buf);
///
/// // No literal `**` leaks — the open bold was repaired closed.
/// let painted: String = buf
///     .cells()
///     .iter()
///     .map(|cell| cell.symbol)
///     .collect();
/// assert!(!painted.contains("**"));
/// assert!(painted.contains("Hi"));
/// ```
#[derive(Debug, Clone)]
pub struct StreamMarkdown<'a> {
    source: &'a str,
    state: Option<&'a StreamMarkdownState>,
    streaming: bool,
    options: RemendOptions,
    width: Option<u16>,
}

impl<'a> StreamMarkdown<'a> {
    /// A streaming view of `source` (streaming on, default
    /// [`RemendOptions`], width inferred from the render area).
    #[must_use]
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            state: None,
            streaming: true,
            options: RemendOptions::default(),
            width: None,
        }
    }

    /// Binds the caller-owned [`StreamMarkdownState`] (the per-block
    /// cache). Without it the widget still renders — it just recomputes
    /// the lines each frame instead of reusing the cache.
    #[must_use]
    pub fn state(mut self, state: &'a StreamMarkdownState) -> Self {
        self.state = Some(state);
        self
    }

    /// Whether the source is still arriving. `true` (default) runs the
    /// repair + segmentation pipeline; `false` is the static fast path (a
    /// finished message: one whole-document
    /// [`rstui_widgets::Markdown`] pass, no repair).
    #[must_use]
    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// Sets the [`RemendOptions`] for the repair pass.
    #[must_use]
    pub fn options(mut self, options: RemendOptions) -> Self {
        self.options = options;
        self
    }

    /// Pins the layout width (otherwise the render area's width is used).
    /// Match this to the width passed to
    /// [`StreamMarkdownState::ingest`] so the cache is reused.
    #[must_use]
    pub fn width(mut self, width: u16) -> Self {
        self.width = Some(width);
        self
    }

    /// The composed display rows for a content area `width` columns wide.
    ///
    /// Reuses the bound [`StreamMarkdownState`]'s cache when it was
    /// `ingest`ed at the same width (the streaming-fast path); otherwise
    /// computes the lines with the same pure pipeline inline. `width` of
    /// zero yields no rows. Never panics.
    #[must_use]
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }
        if let Some(state) = self.state {
            let cache = state.cache();
            if cache.width == width && !cache.entries.is_empty() {
                return cache.lines();
            }
        }
        // No usable cache: compute inline (pure, just not memoised).
        if self.streaming {
            let repaired = remend(self.source, &self.options);
            let open_fence = last_block_has_open_fence(&repaired);
            let blocks = parse_into_blocks(&repaired);
            let mut out = Vec::new();
            for (position, source) in blocks.iter().enumerate() {
                let is_last = position + 1 == blocks.len();
                out.extend(render_block(source, width, is_last && open_fence));
            }
            out
        } else {
            Markdown::new(self.source.to_string()).lines(width)
        }
    }
}

impl Widget for StreamMarkdown<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let width = self.width.unwrap_or(area.width);
        let rows = self.lines(width);
        let max_rows = area.height as usize;
        for (row_offset, line) in rows.into_iter().take(max_rows).enumerate() {
            let y = area.top().saturating_add(row_offset as u16);
            if y >= area.bottom() {
                break;
            }
            let mut x = area.left();
            let line_style = line.style;
            for span in &line.spans {
                let style = line_style.patch(span.style);
                for character in span.content.chars() {
                    if x >= area.right() {
                        break;
                    }
                    buf.set_cell(rstui_core::Position::new(x, y), character, style);
                    x = x.saturating_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstui_core::{Buffer, Position, Rect, Widget};

    /// `remend` with the default options — the common test shape.
    fn fix(text: &str) -> String {
        remend(text, &RemendOptions::default())
    }

    /// `remend` in text-only link mode.
    fn fix_text_only(text: &str) -> String {
        remend(
            text,
            &RemendOptions {
                link_mode: LinkMode::TextOnly,
                ..RemendOptions::default()
            },
        )
    }

    /// `remend` with inline KaTeX opted in.
    fn fix_inline_katex(text: &str) -> String {
        remend(
            text,
            &RemendOptions {
                inline_katex: true,
                ..RemendOptions::default()
            },
        )
    }

    // ---- The input→output table (ported from streamdown __tests__). ----

    #[test]
    fn basic_input_is_unchanged() {
        assert_eq!(fix(""), "");
        let plain = "This is plain text without any markdown";
        assert_eq!(fix(plain), plain);
    }

    #[test]
    fn bold_table() {
        for (input, want) in [
            ("Text with **bold", "Text with **bold**"),
            ("**incomplete", "**incomplete**"),
            ("Text with **bold text**", "Text with **bold text**"),
            ("**bold1** and **bold2**", "**bold1** and **bold2**"),
            ("**first** and **second", "**first** and **second**"),
            ("Here is some **bold tex", "Here is some **bold tex**"),
            // Half-complete closing marker (#313).
            ("**xxx*", "**xxx**"),
            ("**bold text*", "**bold text**"),
            ("Text with **bold*", "Text with **bold**"),
            ("This is **bold text*", "This is **bold text**"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn italic_table() {
        for (input, want) in [
            ("Text with __italic", "Text with __italic__"),
            ("__incomplete", "__incomplete__"),
            ("Text with __italic text__", "Text with __italic text__"),
            ("__first__ and __second", "__first__ and __second__"),
            ("__xxx_", "__xxx__"),
            ("__bold text_", "__bold text__"),
            ("Text with __bold_", "Text with __bold__"),
            ("Text with *italic", "Text with *italic*"),
            ("*incomplete", "*incomplete*"),
            ("Text with *italic text*", "Text with *italic text*"),
            ("**bold** and *italic", "**bold** and *italic*"),
            ("234234*123", "234234*123"),
            ("hello*world", "hello*world"),
            ("test*123*test", "test*123*test"),
            (
                "*italic with some*var*name inside",
                "*italic with some*var*name inside*",
            ),
            (
                "\\*escaped asterisk and *italic",
                "\\*escaped asterisk and *italic*",
            ),
            ("abc*123", "abc*123"),
            ("Text with _italic", "Text with _italic_"),
            ("_incomplete", "_incomplete_"),
            ("__bold__ and _italic", "__bold__ and _italic_"),
            (
                "Text with \\_escaped underscore",
                "Text with \\_escaped underscore",
            ),
            (
                "some\\_text_with_underscores",
                "some\\_text_with_underscores",
            ),
            ("café_price", "café_price"),
            ("some_variable_name", "some_variable_name"),
            ("_start with underscore", "_start with underscore_"),
            ("Text with _italic\n", "Text with _italic_\n"),
            ("_incomplete\n\n", "_incomplete_\n\n"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn bold_italic_table() {
        for (input, want) in [
            ("Text with ***bold-italic", "Text with ***bold-italic***"),
            ("***incomplete", "***incomplete***"),
            (
                "Text with ***bold and italic text***",
                "Text with ***bold and italic text***",
            ),
            (
                "***first*** and ***second***",
                "***first*** and ***second***",
            ),
            ("***first*** and ***second", "***first*** and ***second***"),
            ("*italic* **bold** ***both", "*italic* **bold** ***both***"),
            ("***Starting bold-italic", "***Starting bold-italic***"),
            ("text ***", "text ***"),
            ("text ****", "text ****"),
            ("text***", "text***"),
            ("***text***", "***text***"),
            // Overlapping bold+italic (#302) — *** is a close, not an open.
            (
                "Combined **bold and *italic*** text",
                "Combined **bold and *italic*** text",
            ),
            (
                "**bold and *italic*** more text",
                "**bold and *italic*** more text",
            ),
            ("**bold and *bold-italic***", "**bold and *bold-italic***"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn inline_code_table() {
        for (input, want) in [
            ("Text with `code", "Text with `code`"),
            ("`incomplete", "`incomplete`"),
            ("Text with `inline code`", "Text with `inline code`"),
            ("`code1` and `code2`", "`code1` and `code2`"),
            (
                "```\ncode block with `backtick\n```",
                "```\ncode block with `backtick\n```",
            ),
            (
                "```javascript\nconst x = `template",
                "```javascript\nconst x = `template",
            ),
            (
                "```python print(\"Hello, Sunnyvale!\")```",
                "```python print(\"Hello, Sunnyvale!\")```",
            ),
            (
                "```python print(\"Hello, Sunnyvale!\")``",
                "```python print(\"Hello, Sunnyvale!\")```",
            ),
            ("```code```", "```code```"),
            ("``````", "``````"),
            ("text``````", "text``````"),
            ("```\nblock\n```\n`inline", "```\nblock\n```\n`inline`"),
            ("\\`not code\\` **bold", "\\`not code\\` **bold**"),
            ("\\` *italic", "\\` *italic*"),
            ("`**bold`", "`**bold`"),
            ("`*italic`", "`*italic`"),
            ("`~~strikethrough`", "`~~strikethrough`"),
            ("`code` **bold", "`code` **bold**"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn code_block_table() {
        for (input, want) in [
            ("```javascript\nconst x = 5;", "```javascript\nconst x = 5;"),
            ("```\ncode here", "```\ncode here"),
            (
                "```javascript\nconst x = 5;\n```",
                "```javascript\nconst x = 5;\n```",
            ),
            (
                "```\nconst str = `template`;\n```",
                "```\nconst str = `template`;\n```",
            ),
            (
                "Some text\n```js\nconsole.log",
                "Some text\n```js\nconsole.log",
            ),
            ("```\ncode\n```\nMore text", "```\ncode\n```\nMore text"),
            (
                "```js\ncode1\n```\n\n```python\ncode2\n```",
                "```js\ncode1\n```\n\n```python\ncode2\n```",
            ),
            ("```python code```", "```python code```"),
            ("```python code\n```", "```python code\n```"),
            // Stray `*` from `[*]` in a mermaid block must not appear.
            (
                "Here's a state diagram:\n\n```mermaid\nstateDiagram-v2\n    [*] --> Idle\n```",
                "Here's a state diagram:\n\n```mermaid\nstateDiagram-v2\n    [*] --> Idle\n```",
            ),
            (
                "```mermaid\nstateDiagram-v2\n    [*] --> Idle\n```\n\nHere is *incomplete italic",
                "```mermaid\nstateDiagram-v2\n    [*] --> Idle\n```\n\nHere is *incomplete italic*",
            ),
            (
                "```css\ncode here\n```\n\n**incomplete bold",
                "```css\ncode here\n```\n\n**incomplete bold**",
            ),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn links_protocol_table() {
        for (input, want) in [
            (
                "Text with [incomplete link",
                "Text with [incomplete link](streamdown:incomplete-link)",
            ),
            (
                "Text [partial",
                "Text [partial](streamdown:incomplete-link)",
            ),
            (
                "Text with [complete link](url)",
                "Text with [complete link](url)",
            ),
            (
                "[link1](url1) and [link2](url2)",
                "[link1](url1) and [link2](url2)",
            ),
            (
                "[outer [nested] text](incomplete",
                "[outer [nested] text](streamdown:incomplete-link)",
            ),
            (
                "[link with [brackets] inside](https://example.com)",
                "[link with [brackets] inside](https://example.com)",
            ),
            (
                "Check out [this lin",
                "Check out [this lin](streamdown:incomplete-link)",
            ),
            (
                "Visit [our site](https://exa",
                "Visit [our site](streamdown:incomplete-link)",
            ),
            (
                "Text [outer [inner",
                "Text [outer [inner](streamdown:incomplete-link)",
            ),
            (
                "Text [outer [inner]",
                "Text [outer [inner]](streamdown:incomplete-link)",
            ),
            (
                "Text with [link and **bold",
                "Text with [link and **bold](streamdown:incomplete-link)",
            ),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn links_text_only_table() {
        for (input, want) in [
            ("Text with [incomplete link", "Text with incomplete link"),
            ("Text [partial", "Text partial"),
            (
                "Text with [complete link](url)",
                "Text with [complete link](url)",
            ),
            ("[outer [nested] text](incomplete", "outer [nested] text"),
            ("Check out [this lin", "Check out this lin"),
            ("Visit [our site](https://exa", "Visit our site"),
            ("Text [outer [inner", "Text outer [inner"),
            ("[foo [bar [baz", "foo [bar [baz"),
            ("Text [outer [inner]", "Text outer [inner]"),
            ("Text ![incomplete image", "Text "),
            ("Text ![alt](http://partial", "Text "),
            ("[link](url) [incomplete", "[link](url) incomplete"),
            ("[text] [incomplete", "[text] incomplete"),
        ] {
            assert_eq!(fix_text_only(input), want, "input={input:?}");
        }
    }

    #[test]
    fn image_table() {
        for (input, want) in [
            ("Text with ![incomplete image", "Text with "),
            ("![partial", ""),
            (
                "Text with ![alt text](image.png)",
                "Text with ![alt text](image.png)",
            ),
            ("See ![the diag", "See "),
            ("![logo](./assets/log", ""),
            ("Text ![outer [inner]", "Text "),
            (
                "textContent ![image](https://img.alicdn.com/imgextra/i4/6000000003603/O1CN01ApW8bQ1cUE8LduPra_!!6000000003603-2-skyky.png)",
                "textContent ![image](https://img.alicdn.com/imgextra/i4/6000000003603/O1CN01ApW8bQ1cUE8LduPra_!!6000000003603-2-skyky.png)",
            ),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn strikethrough_table() {
        for (input, want) in [
            ("Text with ~~strike", "Text with ~~strike~~"),
            ("~~incomplete", "~~incomplete~~"),
            (
                "Text with ~~strikethrough text~~",
                "Text with ~~strikethrough text~~",
            ),
            ("~~strike1~~ and ~~strike2~~", "~~strike1~~ and ~~strike2~~"),
            ("~~first~~ and ~~second", "~~first~~ and ~~second~~"),
            ("~~xxx~", "~~xxx~~"),
            ("~~strike text~", "~~strike text~~"),
            ("Text with ~~strike~", "Text with ~~strike~~"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn single_tilde_table() {
        for (input, want) in [
            ("20~25°C", "20\\~25°C"),
            ("20~25°C。20~25°C", "20\\~25°C。20\\~25°C"),
            ("foo~bar", "foo\\~bar"),
            ("~~strikethrough~~", "~~strikethrough~~"),
            ("~hello", "~hello"),
            ("hello~", "hello~"),
            ("hello ~ world", "hello ~ world"),
            ("```\n20~25\n```", "```\n20~25\n```"),
            ("`20~25`", "`20~25`"),
            ("20~25 and ~~strike", "20\\~25 and ~~strike~~"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
        assert_eq!(
            remend(
                "20~25°C",
                &RemendOptions {
                    single_tilde: false,
                    ..RemendOptions::default()
                }
            ),
            "20~25°C"
        );
    }

    #[test]
    fn comparison_operator_table() {
        for (input, want) in [
            ("- > 25: rich", "- \\> 25: rich"),
            ("* > 25: rich", "* \\> 25: rich"),
            ("+ > 25: rich", "+ \\> 25: rich"),
            ("1. > 25: rich", "1. \\> 25: rich"),
            ("2) > 10: high", "2) \\> 10: high"),
            ("  - > 25: rich", "  - \\> 25: rich"),
            ("    - > 5: expensive", "    - \\> 5: expensive"),
            ("- >= 10: high", "- \\>= 10: high"),
            ("- > $100: expensive", "- \\> $100: expensive"),
            ("> Some blockquote", "> Some blockquote"),
            ("> 25 is a number", "> 25 is a number"),
            ("- > Some quoted text", "- > Some quoted text"),
            (">25", ">25"),
            ("```\n- > 25: in code\n```", "```\n- > 25: in code\n```"),
            ("- >25: rich", "- \\>25: rich"),
            ("- > 25: **bold", "- \\> 25: **bold**"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
        assert_eq!(
            remend(
                "- > 25: rich",
                &RemendOptions {
                    comparison_operators: false,
                    ..RemendOptions::default()
                }
            ),
            "- > 25: rich"
        );
    }

    #[test]
    fn html_tag_table() {
        for (input, want) in [
            ("Hello <div", "Hello"),
            ("Hello <custom", "Hello"),
            ("Text <MyComponent", "Text"),
            ("Hello </div", "Hello"),
            ("<div>content</di", "<div>content"),
            ("Hello <div class=\"foo", "Hello"),
            ("Hello <div>", "Hello <div>"),
            ("<div>content</div>", "<div>content</div>"),
            ("<br/>", "<br/>"),
            ("3 < 5", "3 < 5"),
            ("x < y", "x < y"),
            ("value <1", "value <1"),
            ("```\n<div\n```", "```\n<div\n```"),
            ("`<div`", "`<div`"),
            ("<div", ""),
            ("Some text here\n\n<casecard", "Some text here"),
            ("<div>Hello</div> <span", "<div>Hello</div>"),
            (
                "<a target=\"_blank\" href=\"https://link.com\">word</a>",
                "<a target=\"_blank\" href=\"https://link.com\">word</a>",
            ),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
        assert_eq!(
            remend(
                "Hello <div",
                &RemendOptions {
                    html_tags: false,
                    ..RemendOptions::default()
                }
            ),
            "Hello <div"
        );
    }

    #[test]
    fn setext_heading_table() {
        for (input, want) in [
            ("here is a list\n-", "here is a list\n-\u{200b}"),
            ("Some text\n--", "Some text\n--\u{200b}"),
            ("Some text\n=", "Some text\n=\u{200b}"),
            ("Some text\n==", "Some text\n==\u{200b}"),
            ("Some text\n---", "Some text\n---"),
            ("Heading\n===", "Heading\n==="),
            ("-", "-"),
            ("\n-", "\n-"),
            ("here is a list\n- ", "here is a list\n-\u{200b}"),
            (
                "here is a list\n- list item 1",
                "here is a list\n- list item 1",
            ),
            (
                "Line 1\nLine 2\nLine 3\n-",
                "Line 1\nLine 2\nLine 3\n-\u{200b}",
            ),
            ("Some text\n  -", "Some text\n  -\u{200b}"),
            (
                "Some text\n- Item 1\n- Item 2",
                "Some text\n- Item 1\n- Item 2",
            ),
            ("Some text\n-x", "Some text\n-x"),
            ("Some text\n----", "Some text\n----"),
            ("**bold text**\n-", "**bold text**\n-\u{200b}"),
            ("`code`\n-", "`code`\n-\u{200b}"),
            ("\n=", "\n="),
            ("Text 1\n-\nText 2\n-", "Text 1\n-\nText 2\n-\u{200b}"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn katex_block_table() {
        for (input, want) in [
            ("Text with $$formula", "Text with $$formula$$"),
            ("$$incomplete", "$$incomplete$$"),
            ("Text with $$E = mc^2$$", "Text with $$E = mc^2$$"),
            (
                "$$formula1$$ and $$formula2$$",
                "$$formula1$$ and $$formula2$$",
            ),
            ("$$first$$ and $$second", "$$first$$ and $$second$$"),
            ("$$x + y = z", "$$x + y = z$$"),
            ("$$formula$", "$$formula$$"),
            ("$$x = y$", "$$x = y$$"),
            ("$$\nx = 1\ny = 2", "$$\nx = 1\ny = 2\n$$"),
            // Single $ is NOT completed (currency-ambiguous) by default.
            ("Text with $formula", "Text with $formula"),
            ("$incomplete", "$incomplete"),
            ("$first$ and $second", "$first$ and $second"),
            ("$$block$$ and $inline", "$$block$$ and $inline"),
            ("Price is \\$100", "Price is \\$100"),
            ("$$$", "$$$$$"),
            ("$$$$", "$$$$"),
            (
                "The variable $x_1$ represents the first element",
                "The variable $x_1$ represents the first element",
            ),
            ("$$x_1 + y_2 = z_3$$", "$$x_1 + y_2 = z_3$$"),
            ("Math expression $x_", "Math expression $x_"),
            ("$$formula_", "$$formula_$$"),
            ("Start _italic with $x_1$", "Start _italic with $x_1$_"),
            (
                "Streamdown uses double dollar signs (`$$`) to delimit mathematical expressions.",
                "Streamdown uses double dollar signs (`$$`) to delimit mathematical expressions.",
            ),
            ("Math: $$x+y and code: `$$`", "Math: $$x+y and code: `$$`$$"),
            ("$$\\mathbf{w}^{*}$$", "$$\\mathbf{w}^{*}$$"),
            (
                "Start *italic with $$x^{*}$$",
                "Start *italic with $$x^{*}$$*",
            ),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn katex_inline_optin_table() {
        for (input, want) in [
            ("Text with $formula", "Text with $formula$"),
            ("$incomplete", "$incomplete$"),
            ("Text with $x^2 + y^2 = z^2$", "Text with $x^2 + y^2 = z^2$"),
            ("$first$ and $second", "$first$ and $second$"),
            ("$$block$$ and $inline", "$$block$$ and $inline$"),
            ("Price is \\$100", "Price is \\$100"),
            (
                "Use `$var` for variables and $formula",
                "Use `$var` for variables and $formula$",
            ),
            ("Inline $x$ and block $$y$$", "Inline $x$ and block $$y$$"),
            ("$$formula$", "$$formula$$"),
            ("$$x = y$", "$$x = y$$"),
        ] {
            assert_eq!(fix_inline_katex(input), want, "input={input:?}");
        }
    }

    #[test]
    fn horizontal_rule_table() {
        for input in [
            "---", "----", "***", "****", "___", "____", "- - -", "* * *", "_ _ _", "-  -  -",
        ] {
            assert_eq!(fix(input), input, "input={input:?}");
        }
        for (input, want) in [
            (
                "Text before\n***\nText after",
                "Text before\n***\nText after",
            ),
            ("Some text\n\n---", "Some text\n\n---"),
            ("Text with **bold", "Text with **bold**"),
            ("Text with --", "Text with --"),
            ("--", "--"),
            ("**", "**"),
            ("Text\n***", "Text\n***"),
            (
                "This is not a --- horizontal rule",
                "This is not a --- horizontal rule",
            ),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn list_table() {
        for (input, want) in [
            (
                "* Item 1\n* Item 2\n* Item 3",
                "* Item 1\n* Item 2\n* Item 3",
            ),
            ("* Single item", "* Single item"),
            (
                "* Parent item\n  * Nested item 1\n  * Nested item 2",
                "* Parent item\n  * Nested item 1\n  * Nested item 2",
            ),
            (
                "* Item with *italic* text\n* Another item",
                "* Item with *italic* text\n* Another item",
            ),
            (
                "* Item with *incomplete italic\n* Another item",
                "* Item with *incomplete italic\n* Another item*",
            ),
            (
                "- Item 1\n- Item 2 with **bol",
                "- Item 1\n- Item 2 with **bol**",
            ),
            ("- __", "- __"),
            ("- **", "- **"),
            ("- __\n- **", "- __\n- **"),
            ("- __ text after", "- __ text after__"),
            ("- ***", "- ***"),
            ("- *", "- *"),
            ("- `", "- `"),
            ("- **text\nmore text", "- **text\nmore text"),
            ("* **content\n* Another item", "* **content\n* Another item"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn mixed_formatting_table() {
        for (input, want) in [
            (
                "**bold** and *italic* and `code` and ~~strike~~",
                "**bold** and *italic* and `code` and ~~strike~~",
            ),
            ("**bold and *italic", "**bold and *italic*"),
            (
                "**bold with *italic* inside**",
                "**bold with *italic* inside**",
            ),
            (
                "Text with [link and **bold",
                "Text with [link and **bold](streamdown:incomplete-link)",
            ),
            ("*italic with **bold", "*italic with **bold***"),
            ("**bold with `code", "**bold with `code**`"),
            ("~~strike with **bold", "~~strike with **bold**~~"),
            ("**bold with $x^2", "**bold with $x^2**"),
            (
                "**bold *italic `code ~~strike",
                "**bold *italic `code ~~strike*`",
            ),
            (
                "**bold *italic* text** and `code`",
                "**bold *italic* text** and `code`",
            ),
            (
                "combined **_bold and italic",
                "combined **_bold and italic_**",
            ),
            ("**_text", "**_text_**"),
            ("_italic and **bold", "_italic and **bold**_"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn streaming_scenarios_table() {
        for (input, want) in [
            ("This is **bold with *ital", "This is **bold with *ital*"),
            ("**bold _und", "**bold _und_**"),
            (
                "# Main Title\n## Subtitle with **emph",
                "# Main Title\n## Subtitle with **emph**",
            ),
            ("> Quote with **bold", "> Quote with **bold**"),
            (
                "| Col1 | Col2 |\n|------|------|\n| **dat",
                "| Col1 | Col2 |\n|------|------|\n| **dat**",
            ),
            ("Text **bold `code", "Text **bold `code**`"),
            ("Here is", "Here is"),
            ("Here is a **bold", "Here is a **bold**"),
            (
                "Here is a **bold statement** about `code",
                "Here is a **bold statement** about `code`",
            ),
            (
                "To use this function, call `getData(",
                "To use this function, call `getData(`",
            ),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn edge_cases_table() {
        for (input, want) in [
            ("Text ending with *", "Text ending with *"),
            ("Text ending with **", "Text ending with **"),
            ("****", "****"),
            ("``", "``"),
            ("**", "**"),
            ("__", "__"),
            ("***", "***"),
            ("*", "*"),
            ("_", "_"),
            ("~~", "~~"),
            ("`", "`"),
            ("** __", "** __"),
            ("\n** __\n", "\n** __\n"),
            ("* _ ~~ `", "* _ ~~ `"),
            ("** ", "**"),
            (" **", " **"),
            ("  **  ", "  **  "),
            ("**text", "**text**"),
            ("`text", "`text`"),
            ("text**", "text**"),
            ("text*", "text*"),
            ("text`", "text`"),
            ("text$", "text$"),
            ("text~~", "text~~"),
            ("text **bold", "text **bold**"),
            ("text\n**bold", "text\n**bold**"),
            ("text\t`code", "text\t`code`"),
            ("**émoji 🎉", "**émoji 🎉**"),
            ("`código", "`código`"),
            ("**&lt;tag&gt;", "**&lt;tag&gt;**"),
            ("3 + 2 - 5 * 0 = ?", "3 + 2 - 5 * 0 = ?"),
            ("5 * 0", "5 * 0"),
            ("x * y", "x * y"),
            ("2 * 3 * 4", "2 * 3 * 4"),
            ("5 * 0 and *italic", "5 * 0 and *italic*"),
            (" ", ""),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
        // Very long text.
        let long = format!("{} **bold", "a".repeat(10_000));
        assert_eq!(fix(&long), format!("{} **bold**", "a".repeat(10_000)));
    }

    #[test]
    fn coverage_gap_table() {
        for (input, want) in [
            ("__content_", "__content__"),
            ("_text**", "_text**_"),
            ("```\n***bold", "```\n***bold"),
            ("```\n***\n```\n***text", "```\n***\n```\n***text***"),
            ("```\n_code\n```\n_text", "```\n_code\n```\n_text_"),
            ("```\n__code\n```\n__text", "```\n__code\n```\n__text__"),
            ("a~~b~~text", "a~~b~~text"),
            ("a~~b~~c~", "a~~b~~c~"),
            ("](partial", "](partial"),
            ("[link](url) _word", "[link](url) _word_"),
            ("func(_arg", "func(_arg_"),
            ("div> _text", "div> _text_"),
            ("3<5 _text", "3<5 _text_"),
            ("<div>\n_text", "<div>\n_text_"),
            ("[link](a_b) _word", "[link](a_b) _word_"),
            ("```\n__content_", "```\n__content_"),
            ("__a__ __b__content_", "__a__ __b__content_"),
        ] {
            assert_eq!(fix(input), want, "input={input:?}");
        }
    }

    #[test]
    fn custom_handlers_run_after_builtins() {
        let bang = RemendHandler {
            name: "bang",
            priority: DEFAULT_HANDLER_PRIORITY,
            transform: |text| format!("{text}!"),
        };
        let options = RemendOptions {
            handlers: vec![bang],
            ..RemendOptions::default()
        };
        // Bold completed first, then the custom handler appends `!`.
        assert_eq!(remend("**bold", &options), "**bold**!");

        // Disabled built-in + custom still runs.
        let options_no_bold = RemendOptions {
            bold: false,
            handlers: options.handlers.clone(),
            ..RemendOptions::default()
        };
        assert_eq!(remend("**bold", &options_no_bold), "**bold!");

        // Empty handler list is a no-op around the built-ins.
        assert_eq!(
            remend(
                "**bold",
                &RemendOptions {
                    handlers: vec![],
                    ..RemendOptions::default()
                }
            ),
            "**bold**"
        );
    }

    #[test]
    fn custom_handler_priority_orders_before_builtins() {
        // A negative-priority custom handler runs *before* setext (15).
        // Assert via output: a custom handler that strips the trailing
        // newline before setext sees it prevents the zero-width-space
        // insertion setext would otherwise add to "x\n-"-shaped input.
        let strip_tail_newline = RemendHandler {
            name: "strip-newline",
            priority: -1,
            transform: |text| text.trim_end_matches('\n').to_string(),
        };
        let options = RemendOptions {
            handlers: vec![strip_tail_newline],
            ..RemendOptions::default()
        };
        // "x\n" → custom strips to "x" before setext could act → "x".
        assert_eq!(remend("x\n", &options), "x");
    }

    #[test]
    fn exported_predicates_match_streamdown() {
        assert!(is_word_character('a'));
        assert!(is_word_character('Z'));
        assert!(is_word_character('5'));
        assert!(is_word_character('_'));
        assert!(!is_word_character(' '));
        assert!(!is_word_character('*'));

        assert!(is_within_fenced_code_block("```\ncode\n```", 5));
        assert!(!is_within_fenced_code_block("before ```code``` after", 2));

        assert!(is_within_math_block("$$x^2$$", 3));
        assert!(!is_within_math_block("before $x$ after", 14));
        // Single `$` inside a `$$` block still reads as in-math.
        assert!(is_within_math_block("$$x$y$$z", 5));

        assert!(is_within_link_or_image_url(
            "[text](http://example.com)",
            10
        ));
        assert!(!is_within_link_or_image_url("before [text](url) after", 2));
        // Index past the end is clamped, never a panic.
        assert!(!is_within_fenced_code_block("abc", 999));
    }

    #[test]
    fn remend_is_idempotent() {
        // The fixed point holds for every repair *except* the one
        // streamdown itself is not idempotent on: deleting an incomplete
        // image leaves a trailing space, which a second pass's
        // trailing-single-space rule then strips (covered explicitly in
        // `image_deletion_then_space_collapse_is_faithful_to_streamdown`).
        let samples = [
            "This is **bol",
            "see [the docs](http",
            "# H\n\n```py\nprint(1)",
            "- > 25: **bold",
            "20~25 and ~~strike",
            "**bold *italic `code ~~strike",
            "here is a list\n-",
            "$$\nx=1\ny=2",
            "***both",
            "combined **_bold and italic",
            "Text [partial",
            "**xxx*",
            "```python code``",
        ];
        let options = RemendOptions::default();
        for sample in samples {
            let once = remend(sample, &options);
            let twice = remend(&once, &options);
            assert_eq!(once, twice, "not idempotent for {sample:?}");
        }
    }

    #[test]
    fn image_deletion_then_space_collapse_is_faithful_to_streamdown() {
        let options = RemendOptions::default();
        // One pass matches streamdown's `images.test.ts` exactly: the
        // incomplete image is removed, the preceding space preserved.
        assert_eq!(remend("Text ![incomplete image", &options), "Text ");
        // A second pass then applies the trailing-single-space rule (this
        // is the one streamdown-faithful non-idempotency) — and a third
        // pass is a stable fixed point.
        let twice = remend("Text ", &options);
        assert_eq!(twice, "Text");
        assert_eq!(remend(&twice, &options), "Text");
    }

    // ---- Segmentation. ----

    #[test]
    fn segmentation_splits_on_blank_lines() {
        let blocks = parse_into_blocks("# Title\n\nFirst para.\n\nSecond para.");
        assert_eq!(blocks, vec!["# Title", "First para.", "Second para."]);
    }

    #[test]
    fn segmentation_keeps_open_fence_whole() {
        // A blank line inside an unterminated fence must not split it.
        let src = "Intro\n\n```py\na = 1\n\nb = 2";
        let blocks = parse_into_blocks(src);
        assert_eq!(blocks, vec!["Intro", "```py\na = 1\n\nb = 2"]);
        assert!(last_block_has_open_fence(src));
    }

    #[test]
    fn segmentation_closes_fence_then_splits() {
        let src = "```py\na = 1\n```\n\nAfter.";
        let blocks = parse_into_blocks(src);
        assert_eq!(blocks, vec!["```py\na = 1\n```", "After."]);
        assert!(!last_block_has_open_fence(src));
    }

    #[test]
    fn segmentation_merges_open_block_math() {
        let src = "Lead\n\n$$\nx = 1\n\ny = 2";
        let blocks = parse_into_blocks(src);
        assert_eq!(blocks, vec!["Lead", "$$\nx = 1\n\ny = 2"]);
    }

    #[test]
    fn segmentation_keeps_footnote_doc_whole() {
        let src = "Here[^1] is a note.\n\n[^1]: the definition.";
        let blocks = parse_into_blocks(src);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], src);
    }

    #[test]
    fn segmentation_merges_unclosed_html_block() {
        let src = "<div\n\nstill open";
        let blocks = parse_into_blocks(src);
        assert_eq!(blocks, vec!["<div\n\nstill open"]);
    }

    #[test]
    fn segmentation_is_total_on_hostile_input() {
        for src in ["", "\n\n\n", "```", "$$", "<a", "[^", "[^]", "\u{0}"] {
            let _ = parse_into_blocks(src); // must not panic
        }
    }

    // ---- Cache. ----

    #[test]
    fn cache_reuses_unchanged_earlier_blocks() {
        let options = RemendOptions::default();
        let mut cache = StreamCache::new();
        cache.ingest("# Title\n\nStreaming bo", 40, &options, true);
        let first_lines = cache.lines();
        assert_eq!(cache.block_count(), 2);

        // The tail grows; the first block's source is unchanged.
        cache.ingest("# Title\n\nStreaming body now done.", 40, &options, true);
        assert_eq!(cache.block_count(), 2);
        let second_lines = cache.lines();
        // The heading row is identical across the two ingests (reused).
        assert_eq!(first_lines[0], second_lines[0]);
    }

    #[test]
    fn cache_width_change_invalidates() {
        let options = RemendOptions::default();
        let mut cache = StreamCache::new();
        cache.ingest("Some **bol", 40, &options, true);
        assert_eq!(cache.width, 40);
        cache.ingest("Some **bol", 20, &options, true);
        assert_eq!(cache.width, 20);
    }

    #[test]
    fn cache_zero_width_clears() {
        let options = RemendOptions::default();
        let mut cache = StreamCache::new();
        cache.ingest("text", 0, &options, true);
        assert_eq!(cache.block_count(), 0);
        assert!(cache.lines().is_empty());
    }

    // ---- Widget snapshot. ----

    /// Render `widget` into a `w`×`h` buffer and return rows as text.
    fn paint<W: Widget>(widget: W, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        widget.render(buf.area(), &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push(buf.get(Position::new(x, y)).unwrap().symbol);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn partial_input_renders_bold_closed_no_literal_markers() {
        // The required snapshot: a partial message renders bold-closed and
        // shows no literal `**`.
        let source = "# Hi\nThis is **bol";
        let options = RemendOptions::default();
        let mut state = StreamMarkdownState::new();
        state.ingest(source, 24, &options, true);

        let painted = paint(
            StreamMarkdown::new(source)
                .state(&state)
                .options(options)
                .width(24),
            24,
            6,
        );
        assert!(
            !painted.contains("**"),
            "literal markers leaked:\n{painted}"
        );
        assert!(painted.contains("Hi"), "heading missing:\n{painted}");
        assert!(painted.contains("bol"), "body missing:\n{painted}");
    }

    #[test]
    fn widget_renders_without_a_bound_state() {
        // No `.state(...)`: it still renders (computes inline).
        let painted = paint(StreamMarkdown::new("plain **te"), 12, 2);
        assert!(!painted.contains("**"));
        assert!(painted.contains("te"));
    }

    #[test]
    fn widget_static_fast_path_skips_repair() {
        // streaming(false): the open `**` is NOT repaired (static path).
        let mut state = StreamMarkdownState::new();
        let options = RemendOptions::default();
        state.ingest("a **b", 10, &options, false);
        let painted = paint(
            StreamMarkdown::new("a **b")
                .state(&state)
                .streaming(false)
                .width(10),
            10,
            2,
        );
        // The literal `**` survives because no remend ran.
        assert!(painted.contains("**"));
    }

    #[test]
    fn widget_zero_area_is_a_no_op() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
        StreamMarkdown::new("# hi").render(Rect::new(0, 0, 0, 0), &mut buf);
        assert!(buf.cells().iter().all(|c| c.symbol == ' '));
    }

    #[test]
    fn widget_is_total_on_hostile_input() {
        for source in ["", "```", "[^1]", "\u{0}\u{0}", "**", "$$\n"] {
            let mut state = StreamMarkdownState::new();
            let options = RemendOptions::default();
            state.ingest(source, 8, &options, true);
            let _ = paint(StreamMarkdown::new(source).state(&state).width(8), 8, 4); // must not panic
        }
    }

    #[test]
    fn settled_flag_round_trips() {
        let mut state = StreamMarkdownState::new();
        assert!(!state.is_settled());
        state.mark_settled();
        assert!(state.is_settled());
        state.mark_settled(); // idempotent
        assert!(state.is_settled());
    }

    #[test]
    fn open_mermaid_fence_renders_as_plain_text() {
        // An unterminated ```mermaid block must not panic and must show
        // the raw source (no diagram until it closes).
        let src = "```mermaid\ngraph TD\nA --> B";
        let blocks = parse_into_blocks(src);
        assert_eq!(blocks.len(), 1);
        assert!(last_block_has_open_fence(src));
        let lines = render_block(&blocks[0], 40, true);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(joined.contains("graph TD"));
    }

    #[test]
    fn closed_mermaid_fence_delegates_to_mermaid_widget() {
        let src = "```mermaid\ngraph TD\nA[Start] --> B[End]\n```";
        let blocks = parse_into_blocks(src);
        assert_eq!(blocks.len(), 1);
        assert!(!last_block_has_open_fence(src));
        // Should delegate to Mermaid (parse succeeds) and produce rows.
        let lines = render_block(&blocks[0], 40, false);
        assert!(!lines.is_empty());
    }
}
