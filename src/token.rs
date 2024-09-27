use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Deserialize, Serialize)]
struct Claims {
    sub: String,
    exp: usize,
}

pub fn create_auth_token(secret: &str) -> Result<String> {
    let exp = Utc::now() + Duration::seconds(60);

    let claims = Claims {
        sub: "AUTH".to_string(),
        exp: exp.timestamp() as usize,
    };

    let Ok(token) = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    ) else {
        return Err("Error creating JWT token".into());
    };

    Ok(token)
}

pub fn verify_auth_token(token: &str, secret: &str) -> Result<()> {
    let Ok(decoded) = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    ) else {
        return Err(Error::InvalidAuthToken);
    };

    if decoded.claims.sub.len() == 0 {
        return Err(Error::InvalidAuthToken);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_token() {
        // Generate token
        let token = create_auth_token("secret").unwrap();
        assert!(token.len() > 0);

        // Validate it back
        let res = verify_auth_token(&token, "secret");
        assert!(res.is_ok());
    }
}
