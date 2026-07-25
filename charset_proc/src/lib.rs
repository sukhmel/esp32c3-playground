use proc_macro::TokenStream;

/// Escape character used to treat the next character literally instead of as markup
const ESCAPE_CHAR: char = '\u{244A}'; // U+244A HEAVY DOUBLE OBJECT OVERLAPPED (⧈)

/// Parses a single row of charset layout by splitting on unescaped '|' and processing each column segment.
/// Goes char by char without trimming, skips whitespace only if NOT preceded by ESCAPE_CHAR.
/// Escape character makes the next character literal - it goes directly into segment regardless of type.
fn parse_row(row: &str, print: bool) -> Vec<String> {
    let mut segments: Vec<String> = Vec::new();
    let mut current_segment = String::new();

    if print {
        eprintln!("[parse_row] Parsing row: {:?}", row);
    }

    let chars: Vec<char> = row.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == ESCAPE_CHAR {
            // Escape character - the next char is literal, keep it regardless of type (pipe or space)
            if i + 1 < chars.len() {
                let escaped_char = chars[i + 1];
                current_segment.push(escaped_char);
                if print {
                    eprintln!("[parse_row] Escape at pos {}, kept literal {:?} in segment", i, escaped_char);
                }
                i += 2; // Skip both escape char and the escaped character
            } else {
                // Escape char at end of string with no next char - skip it
                if print {
                    eprintln!("[parse_row] Trailing escape char at end, skipping");
                }
                i += 1;
            }
        } else if chars[i] == ' ' {
            // Whitespace - skip it (not preceded by escape since we handle escapes above)
            if print {
                eprintln!("[parse_row] Space at pos {}, skipped", i);
            }
            i += 1;
        } else if chars[i] == '|' {
            // Unescaped pipe is a separator - push current segment and clear
            segments.push(current_segment.clone());
            if print {
                eprintln!("[parse_row] Pipe (separator) at pos {}, pushed segment {:?}", i, current_segment);
            }
            current_segment.clear();
            i += 1;
        } else {
            // Regular character - always keep
            current_segment.push(chars[i]);
            if print {
                eprintln!("[parse_row] Regular char {:?} at pos {}", chars[i], i);
            }
            i += 1;
        }
    }

    // Don't forget the last segment (even if empty due to trailing escape or other reasons)
    segments.push(current_segment.clone());

    if print {
        eprintln!("[parse_row] Final raw segments: {:?}", segments);
    }
    segments
}

/// Core logic that transforms graphically laid out charset to array of strings.
/// Handles empty string rows as separators between column groups.
fn make_charset_impl(rows: &[&str]) -> Vec<String> {
    if rows.is_empty() {
        return vec![];
    }

    // Check if any row is an empty string (acts as group separator)
    let has_separator = rows.iter().any(|r| *r == "");
    
    if !has_separator {
        // No separators - merge all columns together
        let parsed_rows: Vec<Vec<String>> = rows.iter().map(|r| parse_row(r, false)).collect();

        let max_cols = parsed_rows.iter().map(|row| row.len()).max().unwrap_or(0);

        if max_cols == 0 {
            return vec![];
        }

        return (0..max_cols)
            .map(|col_idx| {
                parsed_rows
                    .iter()
                    .filter_map(|row| row.get(col_idx))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect();
    } else {
        // There are separators - split into groups and process each group separately
        let mut result = Vec::new();
        
        // Find ranges of non-empty rows (groups separated by empty rows)
        let mut group_start = 0;
        for i in 0..=rows.len() {
            if i == rows.len() || rows[i] == "" {
                if i > group_start {
                    // Process this group of rows
                    let group_rows: Vec<&str> = rows[group_start..i].to_vec();
                    let parsed_rows: Vec<Vec<String>> = group_rows.iter().map(|r| parse_row(r, false)).collect();

                    let max_cols = parsed_rows.iter().map(|row| row.len()).max().unwrap_or(0);

                    for col_idx in 0..max_cols {
                        let column: String = parsed_rows
                            .iter()
                            .filter_map(|row| row.get(col_idx))
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("");
                        result.push(column);
                    }
                }
                group_start = i + 1; // Skip the separator (empty row)
            }
        }
        
        return result;
    }
}

/// Procedural macro that converts graphical charset layout into a static array.
#[proc_macro]
pub fn make_charset_static(item: TokenStream) -> TokenStream {
    let input = item.to_string();

    // Parse the input - it should be comma-separated string literals
    let mut rows: Vec<String> = Vec::new();

    // Simple parser for string literals in token stream representation
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        if chars[i] == '"' {
            // Start of a string literal - parse it properly handling escapes
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        _ => s.push(chars[i]),
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            rows.push(s);
            if i < chars.len() && chars[i] == '"' {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    let result = make_charset_impl(&rows.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    
    if result.is_empty() {
        return "[]".parse().unwrap();
    }
    
    // Build the output manually to avoid dependency on quote crate
    let mut output_str = String::from("[");
    for (idx, s) in result.iter().enumerate() {
        if idx > 0 {
            output_str.push(',');
        }
        output_str.push('"');
        for c in s.chars() {
            match c {
                '\n' => output_str.push_str("\\n"),
                '\r' => output_str.push_str("\\r"),
                '\t' => output_str.push_str("\\t"),
                '\\' => output_str.push_str("\\\\"),
                '"' => output_str.push_str("\\\""),
                _ => output_str.push(c),
            }
        }
        output_str.push('"');
    }
    output_str.push(']');

    output_str.parse().unwrap()
}
