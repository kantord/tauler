//! Parsing a Package's import specifier. See ADR 0041: `@gh/<owner>/<repo>` only —
//! no subpath in v1.

/// `@gh/<owner>/<repo>` → `(owner, repo)`. Anything else — no `@gh/` prefix, a
/// missing owner or repo, an empty segment, or a subpath after `repo` (not
/// supported in v1, see ADR 0041) — is `None`, not a git Package specifier.
pub fn parse(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("@gh/")?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((owner, repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_owner_and_repo() {
        assert_eq!(parse("@gh/foo/bar"), Some(("foo", "bar")));
    }

    #[test]
    fn rejects_specifiers_without_the_gh_prefix() {
        assert_eq!(parse("./Foo.jsx"), None);
        assert_eq!(parse("@ui/card"), None);
    }

    #[test]
    fn rejects_a_subpath_after_repo() {
        assert_eq!(parse("@gh/foo/bar/baz"), None);
    }

    #[test]
    fn rejects_a_missing_repo() {
        assert_eq!(parse("@gh/foo"), None);
        assert_eq!(parse("@gh/foo/"), None);
    }

    #[test]
    fn rejects_an_empty_owner() {
        assert_eq!(parse("@gh//bar"), None);
    }
}
