use super::*;

fn text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            Block::Text(runs) => runs.iter().map(|run| run.text.as_str()).collect(),
            Block::Table(table) => table
                .cells
                .iter()
                .map(|cell| text(&cell.blocks))
                .collect::<Vec<_>>()
                .join("|"),
        })
        .collect()
}

fn tables(document: &Document) -> Vec<&Table> {
    document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Table(table) => Some(table),
            Block::Text(_) => None,
        })
        .collect()
}

#[test]
fn entity_info_preserves_sections_cells_and_colors() {
    let document = Document::parse(
        "<c \"#FFFF66\">Entity Info</c><br>\
         <table><tbody><tr><td>Name</td><td>A test NPC</td></tr>\
         <tr><td>Details</td><td>長い説明 with words to wrap</td></tr></tbody></table>\
         <c yellow>Stats</c><br><table><thead><tr><th>HP</th><th>MP</th></thead>\
         <tbody><tr><td>12</td><td>34</td></tbody></table>",
    );
    assert_eq!(
        text(&document.blocks),
        "Entity Info\nName|A test NPC|Details|長い説明 with words to wrapStats\nHP|MP|12|34"
    );
    let tables = tables(&document);
    assert_eq!(tables.len(), 2);
    for table in tables {
        assert_eq!(table.columns, 2);
        assert_eq!(
            table
                .cells
                .iter()
                .map(|cell| (cell.row, cell.column))
                .collect::<Vec<_>>(),
            [(0, 0), (0, 1), (1, 0), (1, 1)]
        );
    }
    let Block::Text(runs) = &document.blocks[0] else {
        panic!("heading")
    };
    assert_eq!(runs[0].color, [255, 255, 102]);
    // Native ignores /TR; record that consumed tag once for this offer.
    assert_eq!(document.gaps, ["Server popup STML closing tag /tr"]);
}

#[test]
fn color_stack_survives_tables_invalid_values_and_mixed_closes() {
    let document = Document::parse(
        "</c><c red>R<c \"#00ff00\">G<c invalid>I</c>G</font>R\
         <table><tr><td><c blue>B</c>R<td>R</table></c>W",
    );
    let Block::Text(runs) = &document.blocks[0] else {
        panic!("text")
    };
    assert_eq!(
        runs.iter()
            .map(|run| (run.text.as_str(), run.color))
            .collect::<Vec<_>>(),
        [("R", [255, 0, 0]), ("GIG", [0, 255, 0]), ("R", [255, 0, 0]),]
    );
    let table = tables(&document)[0];
    let Block::Text(runs) = &table.cells[0].blocks[0] else {
        panic!("cell")
    };
    assert_eq!(
        runs.iter()
            .map(|run| (run.text.as_str(), run.color))
            .collect::<Vec<_>>(),
        [("B", [0, 0, 255]), ("R", [255, 0, 0]),]
    );
    let Some(Block::Text(runs)) = document.blocks.last() else {
        panic!("tail")
    };
    assert_eq!(runs[0].color, WHITE);
    assert_eq!(runs[0].text, "W");
    assert_eq!(color("GREY"), Some([128, 128, 128]));
    assert_eq!(color("aqua"), Some([0, 255, 255]));
    assert_eq!(color("#12"), None);
    assert_eq!(color("#ff00zz"), None);
    let invalid_attribute = Document::parse("<c blue><c color='#ff0000'>B</c></c>");
    let Block::Text(runs) = &invalid_attribute.blocks[0] else {
        panic!("text")
    };
    assert_eq!(runs[0].color, [0, 0, 255]);
}

#[test]
fn unknown_openings_stay_literal_closings_drop_and_entities_are_inert() {
    let document = Document::parse(
        "<p>Hello<br/> <c red>traveler</c></p>\
         <a href=\"game:private-value\">Visit</a> &lt;br&gt;\
         <script>text</script><> &QUOT;&NBSP;&other;",
    );
    assert_eq!(
        text(&document.blocks),
        "\nHello\n travelerVisit <br><script>text<> \" &other;"
    );
    assert!(
        document
            .gaps
            .iter()
            .any(|gap| gap.ends_with("closing tag /script"))
    );
    assert!(
        document
            .gaps
            .iter()
            .any(|gap| gap.ends_with("a attribute href"))
    );
    assert!(
        document
            .gaps
            .iter()
            .all(|gap| !gap.contains("private-value"))
    );
    assert_eq!(
        text(&Document::parse("2 < 3 & unknown; <c").blocks),
        "2 < 3 & unknown; <c"
    );
    assert_eq!(text(&Document::parse("").blocks), "");
    assert_eq!(text(&Document::parse("A</>B").blocks), "AB");
}

#[test]
fn spans_skip_occupied_slots_with_or_without_row_closings() {
    let document = Document::parse(
        "<table><tr><td rowspan='2'>A<td colspan=2>B<tr><td>C<td>D</tr>\
         <tr><td colspan=3>E</table>",
    );
    let table = tables(&document)[0];
    assert_eq!(table.columns, 3);
    assert_eq!(
        table
            .cells
            .iter()
            .map(|cell| (cell.row, cell.column, cell.colspan, cell.rowspan))
            .collect::<Vec<_>>(),
        [
            (0, 0, 1, 2),
            (0, 1, 2, 1),
            (1, 1, 1, 1),
            (1, 2, 1, 1),
            (2, 0, 3, 1)
        ]
    );
}

#[test]
fn unsupported_table_attributes_and_formatting_are_logged_without_values() {
    let document = Document::parse(
        "<body bgcolor='private-background'><table width=400 border=1 cellpadding=4>\
         <colgroup><col width=100></colgroup><caption>Caption</caption>\
         <tbody><tr><td bgcolor='private-cell' width=200 height=30 colspan=0 rowspan=65535>\
         <font color=teal face='private-font'><b>Body</b><u>U</u></font>\
         </td></tr></tbody></table></body>",
    );
    for suffix in [
        "body attribute bgcolor",
        "table attribute width",
        "table attribute border",
        "table attribute cellpadding",
        "td attribute bgcolor",
        "td attribute width",
        "td attribute height",
        "font attribute face",
        "tag b formatting",
        "tag u formatting",
        "td attribute colspan outside layout limit",
        "td attribute rowspan outside layout limit",
    ] {
        assert!(
            document
                .gaps
                .contains(&format!("Server popup STML {suffix}")),
            "{suffix}"
        );
    }
    assert!(document.gaps.iter().all(|gap| !gap.contains("private-")));
    assert!(text(&document.blocks).contains("BodyU"));
}

#[test]
fn nested_tables_and_unclosed_cells_keep_text() {
    let document = Document::parse(
        "before<table><tbody><tr><td>outer<table><tr><td>inner</table>tail<td>last",
    );
    assert_eq!(text(&document.blocks), "beforeouterinnertail|last");
    assert_eq!(tables(&document)[0].columns, 2);
}
