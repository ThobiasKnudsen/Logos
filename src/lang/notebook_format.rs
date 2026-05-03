//! Parse a `.logos` notebook file into its constituent cells.
//!
//! A `.logos` file is itself valid Logos source: a top-level block whose
//! bindings (`logos_version`, `cells`) describe the document. Each entry of
//! `cells` is an anonymous record of bindings — `plot_color` (a 4-tuple of
//! normalized floats) and `content` (raw Logos code, never a string).
//!
//! Parsing reuses the language's lexer + parser; we then walk the resulting
//! IR, slice the source by the byte spans on `content`'s value, and dedent
//! the structural indent the saver added so any indentation the user wrote
//! inside the cell round-trips unchanged.
//!
//! This module is intentionally free of `ui::theme` dependencies so it can
//! be exercised from `lang`-level tests (notably WGSL-codegen tests on the
//! shipped examples).
//!
//! See `session::notebook_view` for the wrapper that adds `Rgba` color
//! conversion and orchestrates save/load against the rest of the editor.

use super::ir::Ir;
use super::parse;

/// Spaces the saver prepends to each non-empty line of a cell's `content`
/// body — chosen so the body sits one level deeper than `content := (`,
/// matching how a hand-author would naturally indent. The loader strips up
/// to this many leading spaces per line, so any *additional* indent the
/// user wrote inside the cell survives the round-trip.
pub const CONTENT_INDENT: &str = "        ";

#[derive(Debug, Clone)]
pub struct LogosCell {
    /// Normalized RGBA in `[0.0, 1.0]` per channel, as written in source.
    /// Out-of-range values are NOT clamped here — that is a UI concern.
    pub color: [f32; 4],
    /// Raw cell text, with the saver's structural indent removed.
    pub content: String,
}

pub fn parse_logos(source: &str) -> Result<Vec<LogosCell>, String> {
    let ir = parse(source).map_err(|e| format!("parse error: {}", e))?;
    let top_items = block_items(&ir);

    let version = find_binding(&top_items, "logos_version")
        .ok_or_else(|| "missing required binding `logos_version`".to_string())?;
    match version {
        Ir::Number { .. } => {}
        _ => return Err("`logos_version` must be a number".into()),
    }

    let cells_value = find_binding(&top_items, "cells")
        .ok_or_else(|| "missing required binding `cells`".to_string())?;
    let cell_items = match cells_value {
        Ir::ArrayLiteral { items, .. } => items,
        _ => return Err("`cells` must be an array".into()),
    };

    let mut out = Vec::with_capacity(cell_items.len());
    for (i, cell_ir) in cell_items.iter().enumerate() {
        out.push(parse_cell(source, cell_ir).map_err(|e| format!("cell {}: {}", i, e))?);
    }
    Ok(out)
}

fn parse_cell(source: &str, ir: &Ir) -> Result<LogosCell, String> {
    let fields = block_items(ir);

    let color_value =
        find_binding(&fields, "plot_color").ok_or_else(|| "missing `plot_color`".to_string())?;
    let color = parse_color(color_value)?;

    let content_value =
        find_binding(&fields, "content").ok_or_else(|| "missing `content`".to_string())?;
    let content = extract_content_text(source, content_value);

    Ok(LogosCell { color, content })
}

/// Treat `ir` as a sequence of statements: an `Ir::Block`'s items, or the
/// node itself wrapped in a single-element slice (the parser collapses a
/// single-item block to its sole expression, so we have to handle both).
fn block_items(ir: &Ir) -> Vec<&Ir> {
    match ir {
        Ir::Block { items, .. } => items.iter().collect(),
        other => vec![other],
    }
}

fn find_binding<'a>(items: &[&'a Ir], name: &str) -> Option<&'a Ir> {
    items.iter().find_map(|item| match item {
        Ir::Binding { name: n, value, .. } if n == name => Some(value.as_ref()),
        _ => None,
    })
}

fn parse_color(ir: &Ir) -> Result<[f32; 4], String> {
    let items = match ir {
        Ir::Tuple { items, .. } => items,
        _ => return Err("`plot_color` must be a 4-tuple of numbers".into()),
    };
    if items.len() != 4 {
        return Err(format!(
            "`plot_color` must have 4 components (got {})",
            items.len()
        ));
    }
    let mut out = [0.0f32; 4];
    for (i, item) in items.iter().enumerate() {
        out[i] = match item {
            Ir::Number { value, .. } => *value as f32,
            _ => return Err("`plot_color` components must be numbers".into()),
        };
    }
    Ok(out)
}

fn extract_content_text(source: &str, value: &Ir) -> String {
    let (start, end) = value.span();
    let raw = source.get(start..end).unwrap_or("");
    let stripped = strip_outer_parens(raw);
    let trimmed = stripped.trim_matches(|c: char| c == '\n' || c == '\r');
    dedent_content(trimmed)
}

fn strip_outer_parens(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('(') && s.ends_with(')') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn dedent_content(text: &str) -> String {
    let limit = CONTENT_INDENT.len();
    let result: String = text
        .split('\n')
        .map(|line| {
            let strip = line
                .chars()
                .take_while(|c| *c == ' ')
                .count()
                .min(limit);
            &line[strip..]
        })
        .collect::<Vec<_>>()
        .join("\n");
    // The saver writes `    ),` after the cell body — that leaves trailing
    // whitespace inside the content parens which becomes an empty (or
    // whitespace-only) final line after dedent. Drop it so the cell text
    // round-trips without acquiring a trailing newline.
    if let Some(idx) = result.rfind('\n') {
        if result[idx + 1..].chars().all(|c| c == ' ' || c == '\t') {
            return result[..idx].to_string();
        }
    }
    result
}

/// Format the in-memory cell list as `.logos` source. Pairs with
/// `parse_logos` — round-tripping is exact for cell text and lossy only for
/// color (quantized through whatever the caller does with `[f32; 4]`).
pub fn serialize_logos<I, T>(cells: I) -> String
where
    I: IntoIterator<Item = (T, [f32; 4])>,
    T: AsRef<str>,
{
    let mut out = String::new();
    out.push_str("logos_version := 0.1\n\n");
    out.push_str("cells := [\n");
    for (text, color) in cells {
        out.push_str("  (\n");
        out.push_str(&format!(
            "    plot_color := ({:.4}, {:.4}, {:.4}, {:.4}),\n",
            color[0], color[1], color[2], color[3]
        ));
        out.push_str("    content := (\n");
        for line in text.as_ref().split('\n') {
            if line.is_empty() {
                out.push('\n');
            } else {
                out.push_str(CONTENT_INDENT);
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push_str("    ),\n");
        out.push_str("  ),\n");
    }
    out.push_str("]\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_cells_array() {
        let src = "logos_version := 0.1\ncells := []\n";
        let cells = parse_logos(src).unwrap();
        assert!(cells.is_empty());
    }

    #[test]
    fn parse_missing_version_errors() {
        let src = "cells := []\n";
        let err = parse_logos(src).unwrap_err();
        assert!(err.contains("logos_version"), "got: {}", err);
    }

    #[test]
    fn round_trip_single_cell() {
        let body = "r := sqrt(x*x + y*y)\nplot((r, r, r, 1.0))";
        let src = serialize_logos([(body, [0.96, 0.55, 0.66, 1.0])]);
        let cells = parse_logos(&src).expect("parses");
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].content, body);
        assert!((cells[0].color[0] - 0.96).abs() < 1e-3);
    }

    #[test]
    fn round_trip_preserves_user_indent() {
        let body = "f(x) := (\n  r := x * x\n  r + 1\n)";
        let src = serialize_logos([(body, [0.5, 0.5, 0.5, 1.0])]);
        let cells = parse_logos(&src).unwrap();
        assert_eq!(cells[0].content, body);
    }

    #[test]
    fn dedent_strips_at_most_indent_width() {
        // Lines with fewer than CONTENT_INDENT (=8) leading spaces are fully
        // stripped; lines with more keep the excess as user-intended indent.
        assert_eq!(
            dedent_content("        a\n          b\nc"),
            "        a\n          b\nc".split('\n').map(|l| {
                let n = l.chars().take_while(|c| *c == ' ').count().min(8);
                &l[n..]
            }).collect::<Vec<_>>().join("\n")
        );
        // Concrete: 8 → 0, 10 → 2, 0 → 0
        assert_eq!(dedent_content("        a\n          b\nc"), "a\n  b\nc");
    }
}
