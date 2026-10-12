//! Spreadsheet-safe CSV export for user-owned history.
//!
//! User-controlled text (animal names, notes, breed guesses) is neutralized
//! against formula injection (a leading `'` is prepended when the value
//! starts with `=`, `+`, `-`, `@`, tab, or CR) and quoted per RFC 4180 when
//! it contains a delimiter, quote, or newline. Numeric measurement columns
//! stay unquoted so spreadsheets keep them numeric.

/// Escape one text field for CSV output.
pub fn csv_text(value: &str) -> String {
    // Neutralize spreadsheet formula injection in user-controlled text.
    let guarded = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{}", value)
    } else {
        value.to_string()
    };
    if guarded.contains([',', '"', '\n', '\r']) || guarded.starts_with('\'') {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// Format an optional float column: empty when absent, else the raw number.
pub fn csv_number(value: Option<f64>) -> String {
    match value {
        Some(v) if v.is_finite() => {
            // One decimal like the API; keeps columns numeric in sheets.
            format!("{:.1}", (v * 10.0).round() / 10.0)
        }
        _ => String::new(),
    }
}

/// Format a required float column.
pub fn csv_number_req(value: f64) -> String {
    csv_number(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_fields_are_neutralized_and_quoted() {
        assert_eq!(csv_text("=cmd|evil"), "\"'=cmd|evil\"");
        assert_eq!(csv_text("+123"), "\"'+123\"");
        assert_eq!(csv_text("-5"), "\"'-5\"");
        assert_eq!(csv_text("@mention"), "\"'@mention\"");
        assert_eq!(csv_text("plain"), "plain");
        assert_eq!(csv_text("a,b"), "\"a,b\"");
        assert_eq!(csv_text("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_text("line1\nline2"), "\"line1\nline2\"");
    }

    #[test]
    fn numbers_stay_numeric() {
        assert_eq!(csv_number_req(448.4), "448.4");
        assert_eq!(csv_number(None), "");
        assert_eq!(csv_number(Some(f64::NAN)), "");
    }
}
