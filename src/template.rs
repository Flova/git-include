//! Jinja templating for sync commit messages, powered by `minijinja`.
//!
//! Templates get the full Jinja expression language — variables, filters,
//! conditionals — e.g.:
//!
//! ```text
//! git config include.commitTemplate \
//!   'chore({{ subdir }}): {{ action }} @ {{ short_commit }}{% if action == "pull" %} (from {{ ref }}){% endif %}'
//! ```
//!
//! The literal sequence `\n` becomes a newline before rendering, so
//! multi-line templates fit in single-line git config values. Undefined
//! variables are a hard error (typos surface immediately instead of
//! silently producing broken messages); callers fall back to the default
//! template with a warning.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use minijinja::{Environment, UndefinedBehavior};

pub fn render(template: &str, vars: &[(&str, String)]) -> Result<String> {
    let template = template.replace("\\n", "\n");
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    let ctx: BTreeMap<&str, &str> = vars.iter().map(|(k, v)| (*k, v.as_str())).collect();
    env.render_str(&template, ctx)
        .context("invalid commit message template")
}

#[cfg(test)]
mod tests {
    use super::render;

    fn vars() -> Vec<(&'static str, String)> {
        vec![
            ("action", "pull".into()),
            ("subdir", "vendor/lib".into()),
            ("short_commit", "abc1234".into()),
        ]
    }

    #[test]
    fn substitutes_variables() {
        assert_eq!(
            render("sync {{ subdir }} to {{ short_commit }}", &vars()).unwrap(),
            "sync vendor/lib to abc1234"
        );
    }

    #[test]
    fn whitespace_inside_tags_is_flexible() {
        assert_eq!(
            render("{{subdir}} {{  subdir  }}", &vars()).unwrap(),
            "vendor/lib vendor/lib"
        );
    }

    #[test]
    fn full_jinja_filters_and_conditionals_work() {
        assert_eq!(
            render("{{ subdir | upper }}", &vars()).unwrap(),
            "VENDOR/LIB"
        );
        assert_eq!(
            render(
                "{% if action == 'pull' %}update{% else %}other{% endif %} {{ subdir }}",
                &vars()
            )
            .unwrap(),
            "update vendor/lib"
        );
    }

    #[test]
    fn undefined_variables_are_an_error() {
        let err = render("x {{ nope }} y", &vars()).unwrap_err();
        assert!(format!("{err:#}").contains("invalid commit message template"));
    }

    #[test]
    fn syntax_errors_are_reported() {
        assert!(render("a {{ subdir", &vars()).is_err());
        assert!(render("{% if %}", &vars()).is_err());
    }

    #[test]
    fn escaped_newlines_become_real_ones() {
        assert_eq!(render("a\\n\\nb", &vars()).unwrap(), "a\n\nb");
    }

    #[test]
    fn no_tags_passthrough() {
        assert_eq!(render("plain text", &vars()).unwrap(), "plain text");
    }
}
