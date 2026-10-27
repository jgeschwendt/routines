//! The document: frontmatter and fenced blocks — `fm_get` and `extract_blocks` of bin/routine.

/// Up to `n` leading spaces off `line`.
fn dedent(line: &str, n: usize) -> &str {
    &line[line.bytes().take(n).take_while(|&b| b == b' ').count()..]
}

/// Each line, cut at its first NUL — where awk's C strings end.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_terminator('\n')
        .map(|l| l.split('\0').next().unwrap_or(l))
}

/// A flat `key: value` from the leading `---` fence pair; the first match wins.
/// A BOM and blanks after a fence, invisible in an editor, never drop the frontmatter.
pub fn frontmatter_get(text: &str, key: &str) -> Option<String> {
    let mut lines = lines(text).map(|l| l.trim_end_matches([' ', '\t', '\r']));
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
    for line in lines(text) {
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
pub(crate) mod tests {
    use super::{extract_blocks, frontmatter_get};
    use std::{fs, path::PathBuf, process::Command};

    /// A fresh, empty dir `name` beside the test binary.
    pub(crate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::current_exe().unwrap().with_file_name(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Each case's text in file `<i>` of a scratch dir named for the first of `funcs`, then one
    /// bash there with bin/routine's own `funcs` (space-separated) sliced out running `each` per
    /// case (`$f` the file, `$a` its arg, `$0` bin/routine); its NUL-ended outputs, in case order.
    /// A one-line function's slice runs on to the next `}` line: name it before the function there.
    pub(crate) fn bash(funcs: &str, each: &str, cases: &[(&str, &str)]) -> (PathBuf, Vec<String>) {
        let dir = scratch(funcs.split(' ').next().unwrap());
        for (i, (text, _)) in cases.iter().enumerate() {
            fs::write(dir.join(i.to_string()), text).unwrap();
        }
        let sed: String = funcs
            .split(' ')
            .flat_map(|f| ["/^", f, "()/,/^}/{p;d;};"])
            .collect();
        let script = format!(
            r#"export LC_ALL=C; eval "$(sed -n '{sed}' "$0")"
            i=0; for a; do f=$((i++)); {each}; printf '\0'; done"#
        );
        let out = Command::new("/bin/bash")
            .args([
                "-c",
                &script,
                concat!(env!("CARGO_MANIFEST_DIR"), "/bin/routine"),
            ])
            .args(cases.iter().map(|c| c.1))
            .current_dir(&dir)
            .output()
            .unwrap()
            .stdout;
        let out = String::from_utf8(out).unwrap();
        (dir, out.split_terminator('\0').map(str::to_owned).collect())
    }

    #[test]
    fn frontmatter_matches_the_table_and_bash() {
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
        let ours: Vec<_> = cases.iter().map(|c| frontmatter_get(c.0, c.1)).collect();
        let pairs: Vec<_> = cases.iter().map(|c| (c.0, c.1)).collect();
        let theirs: Vec<_> = (bash("fm_get", r#"fm_get "$f" "$a""#, &pairs).1)
            .into_iter()
            .map(|v| v.strip_suffix('\n').map(str::to_owned))
            .collect();
        let want: Vec<_> = cases.iter().map(|c| c.2.map(str::to_owned)).collect();
        assert_eq!((&ours, &theirs), (&want, &want));
    }

    #[test]
    fn blocks_match_the_table_and_bash() {
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
        let ours: Vec<_> = cases.iter().map(|c| extract_blocks(c.0)).collect();
        let texts: Vec<_> = cases.iter().map(|c| (c.0, "")).collect();
        let (dir, counts) = bash(
            "extract_blocks",
            r#"mkdir "$f.d"; extract_blocks "$f" "$f.d""#,
            &texts,
        );
        let theirs: Vec<Vec<_>> = (counts.iter().enumerate())
            .map(|(i, n)| {
                (1..=n.trim_end().parse().unwrap())
                    .map(|k: u32| fs::read_to_string(dir.join(format!("{i}.d/{k:03}.sh"))).unwrap())
                    .collect()
            })
            .collect();
        let want: Vec<Vec<_>> = cases
            .iter()
            .map(|c| c.1.iter().map(|&b| b.to_owned()).collect())
            .collect();
        assert_eq!((&ours, &theirs), (&want, &want));
    }
}
