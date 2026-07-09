//! Minimal Jinja-style templating for sync commit messages.
//!
//! Supports exactly one construct: `{{ variable }}` substitution. The
//! literal sequence `\n` becomes a newline, so multi-line templates can be
//! stored in single-line git config values:
//!
//! ```text
//! git config include.commitTemplate 'chore: {{ action }} {{ subdir }}\n\nsynced to {{ short_commit }}'
//! ```
//!
//! Unknown variables are left in place (making typos visible in the commit
//! message instead of silently vanishing).

pub fn render(template: &str, vars: &[(&str, String)]) -> String {
    let template = template.replace("\\n", "\n");
    let mut out = String::with_capacity(template.len());
    let mut rest = template.as_str();
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            // Unterminated tag: keep it verbatim.
            out.push_str("{{");
            rest = after;
            continue;
        };
        let key = after[..end].trim();
        match vars.iter().find(|(k, _)| *k == key) {
            Some((_, value)) => out.push_str(value),
            None => {
                out.push_str("{{");
                out.push_str(&after[..end]);
                out.push_str("}}");
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::render;

    fn vars() -> Vec<(&'static str, String)> {
        vec![
            ("subdir", "vendor/lib".into()),
            ("short_commit", "abc1234".into()),
        ]
    }

    #[test]
    fn substitutes_variables() {
        assert_eq!(
            render("sync {{ subdir }} to {{ short_commit }}", &vars()),
            "sync vendor/lib to abc1234"
        );
    }

    #[test]
    fn whitespace_inside_tags_is_flexible() {
        assert_eq!(
            render("{{subdir}} {{  subdir  }}", &vars()),
            "vendor/lib vendor/lib"
        );
    }

    #[test]
    fn unknown_variables_stay_visible() {
        assert_eq!(render("x {{ nope }} y", &vars()), "x {{ nope }} y");
    }

    #[test]
    fn escaped_newlines_become_real_ones() {
        assert_eq!(render("a\\n\\nb", &vars()), "a\n\nb");
    }

    #[test]
    fn unterminated_tag_is_kept_verbatim() {
        assert_eq!(render("a {{ subdir", &vars()), "a {{ subdir");
    }

    #[test]
    fn no_tags_passthrough() {
        assert_eq!(render("plain text", &vars()), "plain text");
    }

    #[test]
    fn adjacent_and_repeated_tags() {
        assert_eq!(
            render("{{ subdir }}{{ subdir }}", &vars()),
            "vendor/libvendor/lib"
        );
    }
}
