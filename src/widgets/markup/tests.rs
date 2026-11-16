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
            &"─".repeat(30),
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
}

#[test]
fn headings_are_bold_in_the_text_color_and_only_level_one_gets_a_muted_rule() {
    let bold = Style::default()
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);
    let lines = render("h1. One\nh2. Two\nh3. Three\nh6. Six", 12);
    assert_eq!(
        text(&lines),
        vec!["One", &"─".repeat(12), "", "Two", "", "Three", "", "Six"]
    );
    for at in [0, 3, 5, 7] {
        assert_eq!(
            lines[at].spans,
            vec![Span::styled(text(&lines)[at].clone(), bold)]
        );
    }
    assert_eq!(lines[1].spans[0].style, muted());
}

#[test]
fn bullets_numbers_and_the_quote_bar_are_muted() {
    let lines = render("* dot\n# one\nbq. said", 20);
    assert_eq!(text(&lines), vec!["• dot", "1. one", "▎ said"]);
    assert_eq!(lines[0].spans[0], Span::styled("• ", muted()));
    assert_eq!(lines[1].spans[0], Span::styled("1. ", muted()));
    assert_eq!(lines[2].spans[0], Span::styled("▎ ", muted()));
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

fn plain(source: &str) -> String {
    let spans = inline(source, text_style());
    assert!(spans.iter().all(|s| s.style == text_style()), "{spans:?}");
    text(&[Line::from(spans)]).remove(0)
}

#[test]
fn a_backslash_before_a_wiki_special_character_drops_out_and_the_character_is_plain() {
    for (source, shown) in [
        ("\\(see DEMO-190)", "(see DEMO-190)"),
        ("a\\)", "a)"),
        ("\\[x\\]", "[x]"),
        ("\\{x\\}", "{x}"),
        ("\\*", "*"),
        ("\\_", "_"),
        ("\\+", "+"),
        ("\\-", "-"),
        ("a\\|b", "a|b"),
        ("\\!", "!"),
        ("\\^", "^"),
        ("\\~", "~"),
        ("\\?", "?"),
    ] {
        assert_eq!(plain(source), shown, "{source}");
    }
}

#[test]
fn a_backslash_before_anything_else_stays_literal() {
    for source in [
        "C:\\dir\\file",
        "\\d+",
        "a\\ b",
        "\\#",
        "\\&",
        "\\.",
        "\\%",
        "x\\",
    ] {
        assert_eq!(plain(source), source, "{source}");
    }
}

#[test]
fn an_escaped_marker_does_not_start_emphasis_a_link_or_code() {
    assert_eq!(plain("\\*not bold\\*"), "*not bold*");
    assert_eq!(plain("\\_not italic\\_"), "_not italic_");
    assert_eq!(plain("\\-not struck\\-"), "-not struck-");
    assert_eq!(plain("\\+not underlined\\+"), "+not underlined+");
    assert_eq!(plain("\\[a|not a url\\]"), "[a|not a url]");
    assert_eq!(plain("\\{{not code}}"), "{{not code}}");
    assert_eq!(plain("\\{color:red}x"), "{color:red}x");
    let spans = inline("*a \\* b*", text_style());
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content, "a * b");
    assert_eq!(spans[0].style, text_style().add_modifier(Modifier::BOLD));
}

#[test]
fn escapes_hold_in_paragraphs_and_list_items_but_not_in_code() {
    let lines = render("\\* not a list\n* \\(item)\n{code}\n\\(x\\)\n{code}", 40);
    assert_eq!(text(&lines), vec!["* not a list", "• (item)", "│ \\(x\\)"]);
}

#[test]
fn an_escaped_pipe_stays_in_its_table_cell() {
    assert_eq!(
        table::parse_table_row("|a\\|b|c \\[d\\|e\\] f|"),
        vec![
            (false, "a\\|b".to_string()),
            (false, "c \\[d\\|e\\] f".to_string())
        ]
    );
    let lines = render("||H||\n|a\\|b|", 40);
    assert_eq!(text(&lines), vec!["H", "───", "a|b"]);
}

#[test]
fn a_bare_link_ends_where_an_escape_begins() {
    let spans = inline("see http://b.io\\) now", text_style());
    assert_eq!(span(&spans, "http://b.io").style, link_style());
    assert_eq!(text(&[Line::from(spans)]), vec!["see http://b.io) now"]);
}

const WIDE_TABLE: &str = "||Query||Cause||Seen||\n\
    |title:* red|wildcard skips the cache and counts every facet again|12,400 hits in 1.9 s, no cache hit|\n\
    |brand:Acme|none, served from cache|4,100 hits in 0.3 s|";

#[test]
fn a_table_fits_at_exactly_its_width_and_becomes_records_one_column_less() {
    let table = "||AAAAA||Bb||\n|aaaaa|bbbbb|";
    let aligned = vec!["AAAAA │ Bb", "──────┼──────", "aaaaa │ bbbbb"];
    assert_eq!(text(&render(table, 14)), aligned);
    assert_eq!(text(&render(table, 13)), aligned);
    assert_eq!(text(&render(table, 12)), vec!["aaaaa", "  Bb  bbbbb"]);
}

#[test]
fn a_table_too_wide_to_align_is_one_record_per_row_with_the_header_as_labels() {
    let lines = render(WIDE_TABLE, 40);
    assert_eq!(
        text(&lines),
        vec![
            "title:* red",
            "  Cause  wildcard skips the cache and",
            "         counts every facet again",
            "  Seen   12,400 hits in 1.9 s, no cache",
            "         hit",
            "",
            "brand:Acme",
            "  Cause  none, served from cache",
            "  Seen   4,100 hits in 0.3 s",
        ]
    );
    let bold = Style::default()
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);
    assert_eq!(lines[0].spans, vec![Span::styled("title:* red", bold)]);
    assert_eq!(lines[1].spans[0], Span::styled("  Cause  ", muted()));
    assert_eq!(lines[2].spans[0], Span::raw("         "));
    assert_eq!(
        lines[3].spans[1],
        Span::styled("12,400 hits in 1.9 s, no cache", text_style())
    );
}

#[test]
fn header_labels_pad_to_the_widest_one_and_a_header_with_many_columns_keeps_every_label() {
    let lines = render(
        "||Key||Owner||Environment||Status||\n|K-1|Alex|production, eu-west-1, primary|open|",
        48,
    );
    assert_eq!(
        text(&lines),
        vec![
            "K-1",
            "  Owner        Alex",
            "  Environment  production, eu-west-1, primary",
            "  Status       open",
        ]
    );
}

#[test]
fn a_label_wider_than_a_third_of_the_pane_is_cut_with_dots() {
    let lines = render(
        "||Key||Environment||\n|K-1|production, eu-west-1, primary|",
        30,
    );
    assert_eq!(
        text(&lines),
        vec![
            "K-1",
            "  Environ...  production,",
            "              eu-west-1,",
            "              primary"
        ]
    );
}

#[test]
fn a_table_without_a_header_row_has_a_bold_first_cell_and_unlabeled_lines() {
    let lines = render(
        "|title:* red|wildcard skips the cache|12,400 hits|\n|b|c|d|",
        20,
    );
    assert_eq!(
        text(&lines),
        vec![
            "title:* red",
            "  wildcard skips the",
            "  cache",
            "  12,400 hits",
            "",
            "b",
            "  c",
            "  d",
        ]
    );
    assert!(lines[0].spans[0]
        .style
        .add_modifier
        .contains(Modifier::BOLD));
}

#[test]
fn record_cells_keep_their_inline_markup() {
    let lines = render(
        "||Who||Note||\n|[~arivera]|run {{make test}} then see http://b.io _now_ please|",
        24,
    );
    assert_eq!(
        text(&lines),
        vec![
            "@arivera",
            "  Note  run make test",
            "        then see",
            "        http://b.io now",
            "        please"
        ]
    );
    let chip = Style::default().fg(Color::Reset).bg(Color::DarkGray);
    assert_eq!(span(&lines[1].spans, "make test").style, chip);
    assert_eq!(span(&lines[3].spans, "http://b.io").style, link_style());
    assert!(span(&lines[3].spans, "now")
        .style
        .add_modifier
        .contains(Modifier::ITALIC));
    assert_eq!(lines[0].spans[0].style.fg, Some(Color::Cyan));
    assert!(lines[0].spans[0]
        .style
        .add_modifier
        .contains(Modifier::BOLD));
}

#[test]
fn record_labels_and_values_measure_wide_characters_by_columns() {
    let lines = render("||Key||Id||名前||\n|K-1|x|日本語のとても長いテキスト|", 16);
    assert_eq!(
        text(&lines),
        vec![
            "K-1",
            "  Id    x",
            "  名前  日本語の",
            "        とても長",
            "        いテキス",
            "        ト"
        ]
    );
}

#[test]
fn a_header_only_table_and_empty_cells_draw_what_they_have() {
    assert_eq!(
        text(&render("||Alpha||Beta||Gamma||", 10)),
        vec!["Alpha", "  Beta", "  Gamma"]
    );
    assert_eq!(
        text(&render("||H||A||B||\n|first| |third|", 14)),
        vec!["first", "  B  third"]
    );
}
