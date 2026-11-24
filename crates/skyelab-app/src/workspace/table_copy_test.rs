use super::*;

fn table(headers: &[&str], rows: &[&[&str]]) -> TableData {
    TableData {
        headers: headers.iter().map(|s| s.to_string()).collect(),
        rows: rows
            .iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect(),
        markdown: String::new(),
        span: None,
    }
}

#[test]
fn tsv_puts_tabs_between_cells_and_flattens_cell_breaks() {
    let t = table(&["Name", "Note"], &[&["a\tb", "line\none"], &["c", "d"]]);
    assert_eq!(tsv(&t), "Name\tNote\na b\tline one\nc\td\n");
}

#[test]
fn html_is_a_real_table_with_escaped_cells() {
    let t = table(&["A", "<b>"], &[&["x & y", "\"q\""]]);
    assert_eq!(
        html(&t),
        "<table><thead><tr><th>A</th><th>&lt;b&gt;</th></tr></thead>\
         <tbody><tr><td>x &amp; y</td><td>&quot;q&quot;</td></tr></tbody></table>"
    );
}

#[test]
fn a_ragged_streaming_row_keeps_what_it_has() {
    let t = table(&["A", "B"], &[&["only"]]);
    assert_eq!(tsv(&t), "A\tB\nonly\n");
    assert!(html(&t).contains("<tr><td>only</td></tr>"));
}
