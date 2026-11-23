//! The document: frontmatter and fenced blocks.

/// Up to `max` leading spaces off `line`.
#[expect(
    clippy::string_slice,
    reason = "the cut counts ASCII spaces: a char boundary"
)]
fn dedent(line: &str, max: usize) -> &str {
    &line[line
        .bytes()
        .take(max)
        .take_while(|&byte| byte == b' ')
        .count()..]
}

/// Each line, cut at its first NUL — where awk's C strings end.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_terminator('\n')
        .map(|line| line.split('\0').next().unwrap_or(line))
}

/// A flat `key: value` from the leading `---` fence pair; the first match wins.
/// A BOM and blanks after a fence, invisible in an editor, never drop the frontmatter.
#[expect(
    clippy::string_slice,
    reason = "the cut is an ASCII blank's offset: a char boundary"
)]
pub fn frontmatter_get(text: &str, key: &str) -> Option<String> {
    let mut lines = lines(text).map(|line| line.trim_end_matches([' ', '\t', '\r']));
    let first = lines.next()?;
    if first.strip_prefix('\u{feff}').unwrap_or(first) != "---" {
        return None;
    }
    for line in lines.take_while(|&line| line != "---") {
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        // ` # …` is a comment; `#` with no blank before it is value
        let cut = value
            .as_bytes()
            .windows(2)
            .position(|pair| matches!(pair, [b' ' | b'\t', b'#']))
            .unwrap_or(value.len());
        let value = value[..cut].trim_matches([' ', '\t']);
        if field.trim_matches([' ', '\t']) == key {
            // `*/15 * * * *` is a YAML alias unquoted, so a quoted value is the natural spelling
            let unquoted = ['"', '\'']
                .into_iter()
                .find_map(|quote| value.strip_prefix(quote)?.strip_suffix(quote));
            return Some(unquoted.unwrap_or(value).to_owned());
        }
    }
    None
}

/// The content of each `sh`/`bash` block, in order. `CommonMark` fences: backtick or
/// tilde runs of 3+, indented 0-3; only a run of the same character at least as long closes. A
/// backtick info string holding a backtick is prose, never an opener; a fence inside an
/// HTML comment is disabled.
pub fn extract_blocks(text: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    // (fence char, run length, opener indent, kept)
    let mut fence: Option<(char, usize, usize, bool)> = None;
    let mut comment = false;
    for line in lines(text) {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let dedented = dedent(line, 3);
        if let Some((ch, len, ind, keep)) = fence {
            let closer = dedented.trim_end_matches([' ', '\t']);
            if closer.len() >= len && closer.chars().all(|glyph| glyph == ch) {
                fence = None;
            } else if let (true, Some(block)) = (keep, blocks.last_mut()) {
                *block += dedent(line, ind);
                block.push('\n');
            }
        } else if comment {
            comment = !line.contains("-->");
        } else if let Some(rest) = dedented
            .strip_prefix("<!")
            .filter(|after| after.starts_with("--"))
        {
            comment = !rest.contains("-->");
        } else if let Some(ch @ ('`' | '~')) = dedented.chars().next() {
            let info = dedented.trim_start_matches(ch);
            let len = dedented.len().saturating_sub(info.len());
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
            fence = Some((ch, len, line.len().saturating_sub(dedented.len()), keep));
        }
    }
    blocks
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a failed unwrap fails the test")]
pub mod tests {
    use super::{extract_blocks, frontmatter_get};
    use std::{env, fs, path::PathBuf, process};

    /// A fresh, empty dir `name.<pid>` beside the test binary: two runs of it never share one.
    pub fn scratch(name: &str) -> PathBuf {
        let dir = env::current_exe()
            .unwrap()
            .with_file_name(format!("{name}.{}", process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn frontmatter_matches_the_table() {
        let cases = [
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
            ("---\nk: a\0b\n---\n", "k", Some("a")), // awk's lines end at a NUL
            ("---\0\nk: v\n---\n", "k", Some("v")),
        ];
        let ours: Vec<_> = cases
            .iter()
            .map(|case| frontmatter_get(case.0, case.1))
            .collect();
        let want: Vec<_> = cases.iter().map(|case| case.2.map(str::to_owned)).collect();
        assert_eq!(ours, want);
    }

    #[test]
    fn blocks_match_the_table() {
        let cases = [
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
            ("```sh\necho a\0b\n```\n", &["echo a\n"]), // awk's lines end at a NUL
            ("```sh\0\nx\n```\n", &["x\n"]),
        ];
        let ours: Vec<_> = cases.iter().map(|case| extract_blocks(case.0)).collect();
        let want: Vec<Vec<_>> = cases
            .iter()
            .map(|case| case.1.iter().map(|&block| block.to_owned()).collect())
            .collect();
        assert_eq!(ours, want);
    }
}
