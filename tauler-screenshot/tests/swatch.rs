//! The swatch scene as a bug check: render every built-in theme and look for
//! theme tokens the theme failed to define.
//!
//! SPEC: after resolution, no class in the rendered tree is a bare shadcn token
//! (`bg-card`, `text-destructive-foreground`...). One that survives was looked up in the
//! theme and not found — and takumi ignores it, so the element renders unstyled with no
//! warning. This is the test that would have caught `Badge`'s `destructive-foreground`
//! being undefined in the shipped default theme.
//!
//! Runs over every built-in, `default` included: it used to fail this on
//! `destructive-foreground`, and gained the token in the same change that added the test.

use tauler_screenshot::{render, Options};

/// Every name the shadcn vocabulary defines, plus the one `Badge` reads that it does not.
const TOKENS: &[&str] = &[
    "background",
    "foreground",
    "card",
    "card-foreground",
    "popover",
    "popover-foreground",
    "primary",
    "primary-foreground",
    "secondary",
    "secondary-foreground",
    "muted",
    "muted-foreground",
    "accent",
    "accent-foreground",
    "destructive",
    "destructive-foreground",
    "border",
    "input",
    "ring",
    "chart-1",
    "chart-2",
    "chart-3",
    "chart-4",
    "chart-5",
    "sidebar",
    "sidebar-foreground",
    "sidebar-primary",
    "sidebar-primary-foreground",
    "sidebar-accent",
    "sidebar-accent-foreground",
    "sidebar-border",
    "sidebar-ring",
];

fn unresolved(name: &str) -> Vec<String> {
    let theme = tauler::theme::builtin::by_name(name).expect("built-in theme");
    let source = tauler::theme::swatch::jsx(&theme);
    let shot = render(
        &source,
        &Options {
            width: 560,
            palette: Some(theme),
            ..Options::default()
        },
    )
    .expect("the swatch scene must evaluate");
    shot.classes
        .into_iter()
        .filter(|c| {
            ["bg-", "text-", "border-", "ring-"]
                .iter()
                .any(|p| c.strip_prefix(p).is_some_and(|rest| TOKENS.contains(&rest)))
        })
        .collect()
}

#[test]
fn no_built_in_leaves_a_token_unresolved() {
    for name in tauler::theme::builtin::names() {
        let left = unresolved(&name);
        assert!(left.is_empty(), "{name}: unresolved tokens {left:?}");
    }
}
