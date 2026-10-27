//! The document: frontmatter and fenced blocks — `fm_get` and `extract_blocks` of bin/routine.

/// Up to `n` leading spaces off `line`.
fn dedent(line: &str, n: usize) -> &str {
    &line[line.bytes().take(n).take_while(|&b| b == b' ').count()..]
}

/// A flat `key: value` from the leading `---` fence pair; the first match wins.
/// A BOM and blanks after a fence, invisible in an editor, never drop the frontmatter.
pub fn frontmatter_get(text: &str, key: &str) -> Option<String> {
    let mut lines = text
        .split_terminator('\n')
        .map(|l| l.trim_end_matches([' ', '\t', '\r']));
    let first = lines.next()?;
    if first.strip_prefix('\u{feff}').unwrap_or(first) != "---" {
        return None;
    }
    for line in lines.take_while(|&l| l != "---") {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        // ` # …` is a comment; `#` with no blank before it is value
        let cut = v
            .as_bytes()
            .windows(2)
            .position(|w| matches!(w, [b' ' | b'\t', b'#']))
            .unwrap_or(v.len());
        let v = v[..cut].trim_matches([' ', '\t']);
        if k.trim_matches([' ', '\t']) == key {
            // `*/15 * * * *` is a YAML alias unquoted, so a quoted value is the natural spelling
            let unquoted = ['"', '\'']
                .into_iter()
                .find_map(|q| v.strip_prefix(q)?.strip_suffix(q));
            return Some(unquoted.unwrap_or(v).to_owned());
        }
    }
    None
}

/// The content of each `sh`/`bash` block, in order. `CommonMark` fences: backtick or
/// tilde runs of 3+, indented 0-3; only a run of the same character at least as long closes. A
/// backtick info string holding a backtick is prose, never an opener; a fence inside an
/// HTML comment is disabled.
#[cfg_attr(not(test), expect(dead_code, reason = "stage 3 (run <name>) wires it"))]
pub fn extract_blocks(text: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    // (fence char, run length, opener indent, kept)
    let mut fence: Option<(char, usize, usize, bool)> = None;
    let mut comment = false;
    for line in text.split_terminator('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let t = dedent(line, 3);
        if let Some((ch, len, ind, keep)) = fence {
            let f = t.trim_end_matches([' ', '\t']);
            if f.len() >= len && f.chars().all(|c| c == ch) {
                fence = None;
            } else if let (true, Some(b)) = (keep, blocks.last_mut()) {
                *b += dedent(line, ind);
                b.push('\n');
            }
        } else if comment {
            comment = !line.contains("-->");
        } else if t.starts_with("<!--") {
            comment = !t[2..].contains("-->");
        } else if let Some(ch @ ('`' | '~')) = t.chars().next() {
            let info = t.trim_start_matches(ch);
            let len = t.len() - info.len();
            if len < 3 || ch == '`' && info.contains('`') {
                continue;
            }
            let tag = info
                .trim_start_matches([' ', '\t'])
                .split([' ', '\t'])
                .next()
                .unwrap_or("");
            let keep = tag == "sh" || tag == "bash";
            if keep {
                blocks.push(String::new());
            }
            fence = Some((ch, len, line.len() - t.len(), keep));
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::{extract_blocks, frontmatter_get};

    #[test]
    fn frontmatter() {
        for (text, key, want) in [
            (
                "---\nschedule: 0 * * * *\n---\n",
                "schedule",
                Some("0 * * * *"),
            ),
            (
                "---\r\nschedule: \"0 * * * *\"\r\n---\r\n",
                "schedule",
                Some("0 * * * *"),
            ), // CRLF, YAML-quoted
            (
                "\u{feff}--- \nrequires: A B\n---\t\n",
                "requires",
                Some("A B"),
            ), // a BOM, blanks after the fences
            ("\n---\nschedule: * * * * *\n---\n", "schedule", None), // a fence that does not lead
            ("# t\n", "schedule", None),
            ("", "schedule", None),
            ("---\ntimeout: 30\n---\n", "schedule", None),
            ("---\na: 1\n---\nb: 2\n", "b", None), // the closing fence ends it
            ("---\nno colon\nb: 2", "b", Some("2")), // no closing fence reads to the end
            ("---\na: 1\na: 2\n---\n", "a", Some("1")), // the first match wins
            ("---\n  a\t:  v  \n---\n", "a", Some("v")),
            ("---\na: b: c\n---\n", "a", Some("b: c")), // the first colon splits
            ("---\na:\n---\n", "a", Some("")),
            ("---\na: x # c\n---\n", "a", Some("x")),
            ("---\na: x\t#c # d\n---\n", "a", Some("x")),
            ("---\na: x#c\n---\n", "a", Some("x#c")), // no blank before the #, no comment
            ("---\na:#c\n---\n", "a", Some("#c")),
            ("---\ntimeout: 08\n---\n", "timeout", Some("08")),
            ("---\na: '*/15 * * * *'\n---\n", "a", Some("*/15 * * * *")),
            ("---\na: \"'x'\"\n---\n", "a", Some("'x'")), // one layer
            ("---\na: \"x'\n---\n", "a", Some("\"x'")),   // unmatched
            ("---\na: \"\n---\n", "a", Some("\"")),
            ("---\na: \"\"\n---\n", "a", Some("")),
        ] {
            assert_eq!(frontmatter_get(text, key).as_deref(), want, "{text:?}");
        }
    }

    #[test]
    fn blocks() {
        for (text, want) in [
            // the suite's fences.md: a ~~~ and a ```` fence hold a ``` line, an inline span
            // opens nothing, an indented fence runs dedented
            (
                "# Fences\n\n~~~markdown\n```sh\nprintf 'example'\n```\n~~~\n\n  ````` ````sh `````; an inline span, never an opener\n\n````sh\ncat <<'X'\n```\nX\n````\n\n1. item\n\n   ```bash\n   printf 'indented\\n'\n   ```\n",
                &["cat <<'X'\n```\nX\n", "printf 'indented\\n'\n"][..],
            ),
            // the suite's comment.md: a fence inside an HTML comment never runs
            (
                "<!--->\n<!-- a one-line comment ends where it opens -->\n<!--\n```sh\ncommented\n```\n-->\n<!-->\n```sh\nuncommented\n```\n",
                &["uncommented\n"],
            ),
            ("```sh\r\necho x\r\n```\r\n", &["echo x\n"]),
            ("```sh\n```\n```bash\n\n```\n", &["", "\n"]),
            ("```bash\na\nb", &["a\nb\n"]), // an unclosed block runs to the end
            (
                "``` sh extra\nx\n```\n~~~sh\ny\n~~~\n```\tbash\nz\n```\n",
                &["x\n", "y\n", "z\n"],
            ),
            ("```shell\nx\n```\n```\ny\n```\n```python\nz\n```\n", &[]),
            (
                "```sh\na\n~~~\n```sh\n``\n`````\nb\n```\n",
                &["a\n~~~\n```sh\n``\n"],
            ),
            ("```sh\n    ```\n   ``` \t\nout\n", &["    ```\n"]), // a closer indents 0-3
            ("    ```sh\nx\n```\n", &[]),                         // so does an opener
            ("  ```sh\n x\n   y\n  ```\n", &["x\n y\n"]), // dedent by the opener's indent only
            ("``\n```sh\n<!--\n```\n", &["<!--\n"]),      // a comment inside a block is content
            ("    <!--\n```sh\nx\n```\n", &["x\n"]),      // a 4-space <!-- is no comment
            ("   <!--\n```sh\nx\n```\n-->\n```sh\ny\n```\n", &["y\n"]),
        ] {
            assert_eq!(extract_blocks(text), want, "{text:?}");
        }
    }
}
