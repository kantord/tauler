pub fn transpile(source: &str) -> Result<String, String> {
    let re = regex::Regex::new(r#"^<(\w+)((?:\s+\w+="[^"]*")*)\s*/>$"#).unwrap();
    if let Some(caps) = re.captures(source) {
        let tag = &caps[1];
        let attrs_raw = caps[2].trim();
        if attrs_raw.is_empty() {
            return Ok(format!("{}({{}})", tag));
        }
        let attrs: String = attrs_raw.split_whitespace().collect::<Vec<_>>().join(",");
        return Ok(format!("{}({{{}}})", tag, attrs));
    }
    Err(format!("Cannot transpile: {}", source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_self_closing_tag_no_attributes() {
        let result = transpile("<Container />");
        assert_eq!(result, Ok("Container({})".to_string()));
    }

    #[test]
    fn test_self_closing_tag_single_string_attribute() {
        let result = transpile("<text tw=\"flex\" />");
        assert_eq!(result, Ok("text({tw=\"flex\"})".to_string()));
    }
}
