//! Telegram Mini App initData verification. Never trust initDataUnsafe or IDs
//! supplied separately by the client.
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub const INIT_DATA_MAX_AGE_SECONDS: u64 = 3600;
const CLOCK_SKEW_SECONDS: u64 = 30;

#[derive(Clone)]
pub struct TelegramAuth {
    secret: Arc<[u8]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "123456:test-only-token";
    const NOW: u64 = 1_700_000_000;
    // Fixed vector generated independently with Python's standard-library
    // hmac/hashlib. Covers Unicode, literal '+', URL escaping and signature.
    const VECTOR: &str = "auth_date=1700000000&query_id=AA%2Btest%2F%3D%3F&signature=test-signature&user=%7B%22id%22%3A7000000001%2C%22first_name%22%3A%22Zo%C3%AB+%2B+Test%22%7D&hash=49d8592d77d99326929f178a707e03ab027df9a592d125b895edb892ad117413";

    fn sign(fields: &[(&str, &str)]) -> String {
        let fields: BTreeMap<_, _> = fields.iter().copied().collect();
        let text = fields
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut key = HmacSha256::new_from_slice(b"WebAppData").unwrap();
        key.update(TOKEN.as_bytes());
        let mut mac = HmacSha256::new_from_slice(&key.finalize().into_bytes()).unwrap();
        mac.update(text.as_bytes());
        let mut query = form_urlencoded::Serializer::new(String::new());
        for (key, value) in fields {
            query.append_pair(key, value);
        }
        query
            .append_pair("hash", &hex::encode(mac.finalize().into_bytes()))
            .finish()
    }

    #[test]
    fn verifies_independent_vector_and_ignores_query_order() {
        let auth = TelegramAuth::new(TOKEN);
        assert_eq!(auth.verify_at(VECTOR, NOW).unwrap(), 7_000_000_001);
        let reversed = VECTOR.split('&').rev().collect::<Vec<_>>().join("&");
        assert_eq!(auth.verify_at(&reversed, NOW).unwrap(), 7_000_000_001);
    }

    #[test]
    fn rejects_tampering_wrong_bot_and_bad_hashes() {
        let auth = TelegramAuth::new(TOKEN);
        for data in [
            VECTOR.replace("7000000001", "7000000002"),
            VECTOR.replace("1700000000", "1700000001"),
            VECTOR.replace("test-signature", "other-signature"),
            VECTOR.replace("hash=49", "hash=zz"),
            VECTOR.split("&hash=").next().unwrap().to_owned(),
            format!("{VECTOR}00"),
            String::new(),
        ] {
            assert!(auth.verify_at(&data, NOW).is_err());
        }
        assert!(
            TelegramAuth::new("different-bot-token")
                .verify_at(VECTOR, NOW)
                .is_err()
        );
    }

    #[test]
    fn bounds_age_and_clock_skew() {
        let auth = TelegramAuth::new(TOKEN);
        assert!(
            auth.verify_at(VECTOR, NOW + INIT_DATA_MAX_AGE_SECONDS)
                .is_ok()
        );
        assert!(
            auth.verify_at(VECTOR, NOW + INIT_DATA_MAX_AGE_SECONDS + 1)
                .is_err()
        );
        assert!(auth.verify_at(VECTOR, NOW - CLOCK_SKEW_SECONDS).is_ok());
        assert!(
            auth.verify_at(VECTOR, NOW - CLOCK_SKEW_SECONDS - 1)
                .is_err()
        );
    }

    #[test]
    fn rejects_duplicate_fields_including_encoded_names() {
        let auth = TelegramAuth::new(TOKEN);
        for extra in [
            "user=%7B%22id%22%3A9%7D",
            "u%73er=x",
            "hash=00",
            "auth_date=1700000000",
            "query_id=other",
        ] {
            assert!(auth.verify_at(&format!("{VECTOR}&{extra}"), NOW).is_err());
        }
    }

    #[test]
    fn rejects_signed_data_without_valid_user_or_auth_date() {
        let auth = TelegramAuth::new(TOKEN);
        for user in [
            "{}",
            "null",
            "not json",
            r#"{"id":0}"#,
            r#"{"id":-1}"#,
            r#"{"id":"7"}"#,
            r#"{"id":1.5}"#,
        ] {
            let data = sign(&[("auth_date", "1700000000"), ("user", user)]);
            assert!(auth.verify_at(&data, NOW).is_err());
        }
        for fields in [
            vec![("auth_date", "1700000000")],
            vec![("user", r#"{"id":7}"#)],
            vec![("auth_date", "invalid"), ("user", r#"{"id":7}"#)],
            vec![("auth_date", "-1"), ("user", r#"{"id":7}"#)],
        ] {
            assert!(auth.verify_at(&sign(&fields), NOW).is_err());
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid or expired Telegram authorization; close and reopen the Mini App")]
pub struct AuthError;

#[derive(Deserialize)]
struct TelegramUser {
    id: i64,
}

impl TelegramAuth {
    pub fn new(bot_token: &str) -> Self {
        let mut key = HmacSha256::new_from_slice(b"WebAppData").expect("HMAC accepts any key size");
        key.update(bot_token.as_bytes());
        Self {
            secret: Arc::from(key.finalize().into_bytes().as_slice()),
        }
    }

    pub fn verify(&self, init_data: &str) -> Result<i64, AuthError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError)?
            .as_secs();
        self.verify_at(init_data, now)
    }

    fn verify_at(&self, init_data: &str, now: u64) -> Result<i64, AuthError> {
        if init_data.is_empty() || init_data.len() > 16384 {
            return Err(AuthError);
        }
        let mut fields = BTreeMap::new();
        for (key, value) in form_urlencoded::parse(init_data.as_bytes()) {
            if fields
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(AuthError);
            }
        }
        let hash = fields.remove("hash").ok_or(AuthError)?;
        let hash = hex::decode(hash).map_err(|_| AuthError)?;
        // Telegram's bot-token HMAC covers every field except hash (including
        // signature, when present). Decode values once and sort by field name.
        let data_check_string = fields
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key size");
        mac.update(data_check_string.as_bytes());
        mac.verify_slice(&hash).map_err(|_| AuthError)?;

        let auth_date: u64 = fields
            .get("auth_date")
            .ok_or(AuthError)?
            .parse()
            .map_err(|_| AuthError)?;
        if auth_date > now.saturating_add(CLOCK_SKEW_SECONDS)
            || now.saturating_sub(auth_date) > INIT_DATA_MAX_AGE_SECONDS
        {
            return Err(AuthError);
        }
        let user: TelegramUser =
            serde_json::from_str(fields.get("user").ok_or(AuthError)?).map_err(|_| AuthError)?;
        if user.id <= 0 {
            return Err(AuthError);
        }
        Ok(user.id)
    }
}
