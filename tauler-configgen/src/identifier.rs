//! A schema-derived name that has passed identifier validation.
//!
//! The only way to get one is [`Identifier::validate`] — nothing downstream can format an
//! unvalidated `&str` into a position that expects a JS identifier. Design record §13.3:
//! names are never escaped, only validated, because there is no transformation that makes
//! an arbitrary string safe in bare-identifier position (a schema property named
//! `}); evil(); (({ ` breaks a destructuring parameter list no matter how the string is
//! quoted). Refusing to construct one at all is the only sound move.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Identifier(String);

impl Identifier {
    pub fn validate(name: &str) -> Result<Self, InvalidIdentifier> {
        let mut chars = name.chars();
        let ok = match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {
                chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
            }
            _ => false,
        };
        if ok {
            Ok(Identifier(name.to_string()))
        } else {
            Err(InvalidIdentifier(name.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("\"{0}\" is not a safe JS identifier — must match ^[A-Za-z_$][A-Za-z0-9_$]*$")]
pub struct InvalidIdentifier(String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_plain_name() {
        assert!(Identifier::validate("BackgroundColor").is_ok());
    }

    #[test]
    fn accepts_leading_underscore_and_dollar() {
        assert!(Identifier::validate("_Foo").is_ok());
        assert!(Identifier::validate("$Foo").is_ok());
    }

    #[test]
    fn rejects_empty_string() {
        assert!(Identifier::validate("").is_err());
    }

    #[test]
    fn rejects_a_leading_digit() {
        assert!(Identifier::validate("1Foo").is_err());
    }

    /// The concrete exploit the design record's skeptic round built: a malicious property
    /// name that breaks out of a destructuring parameter list. No escaping function fixes
    /// this — validation must refuse it outright, which is the entire reason `Identifier`
    /// exists instead of a plain `String`.
    #[test]
    fn rejects_the_identifier_breakout_payload() {
        let payload = r#"}); globalThis.sh`rm -rf ~`; (({ "#;
        assert!(Identifier::validate(payload).is_err());
    }

    #[test]
    fn rejects_a_name_with_spaces() {
        assert!(Identifier::validate("background color").is_err());
    }
}
