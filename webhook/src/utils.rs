pub fn valid_webhook_path(value: &str) -> bool {
    if value.len() > 1 && value.starts_with("/") {
        let chars: Vec<char> = value.chars().collect();

        // Next character after "/" must be an alpha-numeric character
        if !chars[1].is_alphanumeric() {
            return false;
        }

        // All characters after this can be either alphanumeric or dash or understore
        for ch in chars.iter().skip(2) {
            if !ch.is_alphanumeric() && *ch != '-' && *ch != '_' {
                return false;
            }
        }

        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_webhook_path() {
        assert_eq!(valid_webhook_path(""), false);
        assert_eq!(valid_webhook_path("/"), false);
        assert_eq!(valid_webhook_path("//"), false);
        assert_eq!(valid_webhook_path("/^_^"), false);
        assert_eq!(valid_webhook_path("/_foo"), false);
        assert_eq!(valid_webhook_path("/-foo"), false);
        assert_eq!(valid_webhook_path("/foo?"), false);
        assert_eq!(valid_webhook_path("/foo"), true);
        assert_eq!(valid_webhook_path("/foo-bar"), true);
        assert_eq!(valid_webhook_path("/foo_bar"), true);
    }
}
