use super::*;

fn text(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

fn span<'a>(spans: &'a [Span<'static>], content: &str) -> &'a Span<'static> {
    spans
        .iter()
        .find(|s| s.content == content)
        .unwrap_or_else(|| panic!("no span {:?} in {:?}", content, spans))
}

#[test]
fn inline_emphasis() {
    let spans = inline("*Stakeholders:* a _b_ -c- +d+ {{e}}", text_style());
    assert!(span(&spans, "Stakeholders:")
        .style
        .add_modifier
        .contains(Modifier::BOLD));
    assert!(span(&spans, "b")
        .style
        .add_modifier
        .contains(Modifier::ITALIC));
    assert!(span(&spans, "c")
        .style
        .add_modifier
        .contains(Modifier::CROSSED_OUT));
    assert!(span(&spans, "d")
        .style
        .add_modifier
        .contains(Modifier::UNDERLINED));
    assert_eq!(
        span(&spans, "e").style,
        Style::default().fg(Color::Reset).bg(Color::DarkGray)
    );
}

#[test]
fn inline_code_is_a_chip_in_the_text_color_on_the_selection_background() {
    let chip = Style::default().fg(Color::Reset).bg(Color::DarkGray);
    for source in ["{{x}}", "`x`"] {
        assert_eq!(inline(source, text_style()), vec![Span::styled("x", chip)]);
    }
    let spans = inline("*bold {{x}}*", text_style());
    assert_eq!(span(&spans, "x").style, chip.add_modifier(Modifier::BOLD));
    let spans = inline("{color:red}{{x}}{color}", text_style());
    assert_eq!(span(&spans, "x").style, chip);
    let spans = inline("{{two words}}", text_style());
    assert_eq!(spans, vec![Span::styled("two words", chip)]);
}

#[test]
fn a_theme_recolors_the_chip_to_its_text_on_its_selection_color() {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    let line = Line::from(inline("ab {{cd}}", text_style()));
    let mut buffer = Buffer::empty(Rect::new(0, 0, 9, 1));
    buffer.set_line(0, 0, &line, 9);
    crate::theme::resolve("nord", &Default::default()).apply(&mut buffer);
    let colors = |x| (buffer[(x, 0)].fg, buffer[(x, 0)].bg);
    let (text, selection) = (Color::Rgb(0xd8, 0xde, 0xe9), Color::Rgb(0x3b, 0x42, 0x52));
    assert_eq!(colors(0), (text, Color::Rgb(0x2e, 0x34, 0x40)));
    assert_eq!(colors(3), (text, selection));
    assert_eq!(colors(4), (text, selection));
    assert_eq!(buffer[(5, 0)].symbol(), " ");
}

#[test]
fn markers_inside_words_stay_plain() {
    for s in [
        "snake_case_name",
        "open-source repos",
        "a * b * c",
        "C++ and x+y+z",
    ] {
        let spans = inline(s, text_style());
        assert_eq!(text(&[Line::from(spans.clone())]), vec![s.to_string()]);
        assert!(spans.iter().all(|s| s.style == text_style()), "{:?}", spans);
    }
}

#[test]
fn nested_emphasis() {
    let spans = inline("*bold _both_*", text_style());
    let both = span(&spans, "both").style.add_modifier;
    assert!(both.contains(Modifier::BOLD | Modifier::ITALIC));
}

#[test]
fn links_and_mentions() {
    let spans = inline(
        "at [10gen/mage|https://github.com/10gen/mage] by [~jdoe], see https://x.io/a.",
        text_style(),
    );
    assert_eq!(
        text(&[Line::from(spans.clone())]),
        vec!["at 10gen/mage (https://github.com/10gen/mage) by @jdoe, see https://x.io/a."]
    );
    assert_eq!(span(&spans, "10gen/mage").style, link_style());
    assert_eq!(span(&spans, "https://x.io/a").style, link_style());
    assert_eq!(span(&spans, "@jdoe").style.fg, Some(Color::Cyan));
}

#[test]
fn brackets_that_are_not_links_stay() {
    let spans = inline("[WIP] thing", text_style());
    assert_eq!(text(&[Line::from(spans)]), vec!["[WIP] thing"]);
}

#[test]
fn colors_icons_images_dashes() {
    let spans = inline(
        "{color:red}hot{color} (/) !shot.png|thumbnail! a -- b --- c",
        text_style(),
    );
    assert_eq!(span(&spans, "hot").style.fg, Some(Color::Red));
    assert_eq!(span(&spans, "✓").style.fg, Some(Color::Green));
    assert_eq!(
        text(&[Line::from(spans)]),
        vec!["hot ✓ [image: shot.png] a – b — c"]
    );
}

#[test]
fn headings_lists_and_rules() {
    let lines = render(
        "h1. Title\nintro\n# one\n# two\n## nested\n# three\n* dot\n** sub\n----",
        30,
    );
    assert_eq!(
        text(&lines),
        vec![
            "Title",
            "intro",
            "1. one",
            "2. two",
            "  1. nested",
            "3. three",
            "• dot",
            "  ◦ sub",
            &"─".repeat(30),
        ]
    );
    assert!(lines[0].spans[0]
        .style
        .add_modifier
        .contains(Modifier::BOLD));
}

#[test]
fn list_items_wrap_with_hanging_indent() {
    let lines = render("* alpha beta gamma delta", 14);
    assert_eq!(text(&lines), vec!["• alpha beta", "  gamma delta"]);
}

#[test]
fn paragraphs_wrap_and_break_long_words() {
    let lines = render(&format!("hi {}", "x".repeat(25)), 20);
    assert_eq!(text(&lines), vec!["hi xxxxxxxxxxxxxxxxx", "xxxxxxxx"]);
}

#[test]
fn code_blocks_are_not_parsed() {
    let lines = render("{code:rust}\nlet *x* = [a|b];\n{code}\nafter", 40);
    assert_eq!(text(&lines), vec!["│ let *x* = [a|b];", "after"]);

    let lines = render("{noformat}raw _text_{noformat}", 40);
    assert_eq!(text(&lines), vec!["│ raw _text_"]);

    let lines = render("```\n*x*\n```", 40);
    assert_eq!(text(&lines), vec!["│ *x*"]);
}

#[test]
fn quotes() {
    let lines = render("bq. quoted\n{quote}\nmulti\n{quote}\nplain", 40);
    assert_eq!(text(&lines), vec!["▎ quoted", "▎ multi", "plain"]);
}

#[test]
fn tables_align_columns() {
    let lines = render("||Name||Owner||\n|PAY|[~arivera]|\n|x|y|", 40);
    assert_eq!(
        text(&lines),
        vec![
            "Name │ Owner",
            "─────┼─────────",
            "PAY  │ @arivera",
            "x    │ y",
        ]
    );
}

#[test]
fn table_cells_keep_link_pipes() {
    let cells = table::parse_table_row("|[a|http://b]|c|");
    assert_eq!(
        cells,
        vec![
            (false, "[a|http://b]".to_string()),
            (false, "c".to_string())
        ]
    );
}

#[test]
fn blank_lines_collapse_and_trail_trimmed() {
    let lines = render("a\n\n\n\nb\n\n", 40);
    assert_eq!(text(&lines), vec!["a", "", "b"]);
}

#[test]
fn forced_line_breaks() {
    let lines = render("one\\\\two", 40);
    assert_eq!(text(&lines), vec!["one", "two"]);
}
