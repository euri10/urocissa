// src/router/claims/claims.rs
use crate::public::structure::album::ResolvedShare;
use chrono::Utc;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Admin,
    Public,
    Share(Box<ResolvedShare>),
}

#[cfg(test)]
mod public_gallery_tests {
    use super::Claims;

    #[test]
    fn public_gallery_claims_never_grant_owner_access() {
        let claims = Claims::new_public();
        assert!(!claims.is_admin());
        let value = serde_json::to_value(&claims).unwrap();
        assert_eq!(value["role"], "public");
        assert!(claims.get_share().is_none());
        assert!(Claims::new_admin().is_admin());
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claims {
    pub role: Role,
    pub exp: u64, // seconds since epoch
}

impl Claims {
    pub fn new_public() -> Self {
        Self {
            role: Role::Public,
            exp: 0,
        }
    }

    pub fn new_admin() -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 14 * 86_400; // 14 days

        Self {
            role: Role::Admin,
            exp,
        }
    }

    pub fn new_share(resolved_share: ResolvedShare) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let exp = (Utc::now().timestamp_millis() / 1000) as u64 + 14 * 86_400; // 14 days

        Self {
            role: Role::Share(Box::new(resolved_share)),
            exp,
        }
    }
    pub fn is_admin(&self) -> bool {
        matches!(self.role, Role::Admin)
    }
    pub fn get_share(self) -> Option<ResolvedShare> {
        match self.role {
            Role::Share(share) => Some(*share),
            Role::Admin | Role::Public => None,
        }
    }

    pub fn encode(&self) -> String {
        use crate::public::structure::config::APP_CONFIG;

        let config = APP_CONFIG.get().unwrap().read().unwrap();
        self.encode_with_key(&config.get_jwt_secret_key())
    }

    pub fn encode_with_key(&self, key: &[u8]) -> String {
        encode(&Header::default(), &self, &EncodingKey::from_secret(key))
            .expect("Failed to generate token")
    }
}
