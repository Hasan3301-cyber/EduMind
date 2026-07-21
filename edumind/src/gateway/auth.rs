use std::sync::{Arc, RwLock};

use axum::http::{HeaderMap, HeaderValue, header};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use thiserror::Error;

use crate::config::{EduMindConfig, types::AuthMode};

/// Authorization role assigned to an authenticated gateway principal.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Student,
    Admin,
}

/// Identity propagated through authenticated HTTP and WebSocket requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthPrincipal {
    pub subject: String,
    pub role: Role,
}

/// Errors deliberately safe to expose to gateway clients.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("authorization header must use the Bearer scheme")]
    InvalidAuthorizationHeader,
    #[error("credentials are invalid")]
    InvalidCredentials,
    #[error("request origin is not allowed")]
    OriginNotAllowed,
    #[error("credentials are required")]
    Required,
    #[error("authentication service is unavailable")]
    Unavailable,
}

/// Validates token and JWT credentials against the live gateway configuration.
#[derive(Clone)]
pub struct AuthService {
    config: Arc<RwLock<EduMindConfig>>,
}

impl AuthService {
    /// Creates an auth service sharing a live configuration handle.
    #[must_use]
    pub fn new(config: Arc<RwLock<EduMindConfig>>) -> Self {
        Self { config }
    }

    /// Authenticates a direct bearer token, such as one supplied in a WebSocket handshake.
    pub fn authenticate(&self, bearer: Option<&str>) -> Result<AuthPrincipal, AuthError> {
        let config = self.config().map_err(|_| AuthError::Unavailable)?;
        match config.gateway.auth.mode {
            AuthMode::None => Ok(AuthPrincipal {
                subject: "local".to_owned(),
                role: Role::Admin,
            }),
            AuthMode::Token => {
                let expected = config
                    .gateway
                    .auth
                    .token
                    .as_deref()
                    .ok_or(AuthError::Unavailable)?;
                let provided = bearer.ok_or(AuthError::Required)?;
                if bool::from(expected.as_bytes().ct_eq(provided.as_bytes())) {
                    Ok(AuthPrincipal {
                        subject: "token".to_owned(),
                        role: Role::Admin,
                    })
                } else {
                    Err(AuthError::InvalidCredentials)
                }
            }
            AuthMode::Jwt => {
                let secret = config
                    .gateway
                    .auth
                    .jwt_secret
                    .as_deref()
                    .ok_or(AuthError::Unavailable)?;
                let bearer = bearer.ok_or(AuthError::Required)?;
                let token = decode::<JwtClaims>(
                    bearer,
                    &DecodingKey::from_secret(secret.as_bytes()),
                    &Validation::new(Algorithm::HS256),
                )
                .map_err(|_| AuthError::InvalidCredentials)?;
                let JwtClaims { sub, role, _exp: _ } = token.claims;
                Ok(AuthPrincipal { subject: sub, role })
            }
        }
    }

    /// Extracts and validates an HTTP authorization header.
    pub fn authenticate_headers(&self, headers: &HeaderMap) -> Result<AuthPrincipal, AuthError> {
        let mode = self
            .config()
            .map_err(|_| AuthError::Unavailable)?
            .gateway
            .auth
            .mode;
        if mode == AuthMode::None {
            return self.authenticate(None);
        }
        self.authenticate(Some(extract_bearer(headers)?))
    }

    /// Validates a browser origin against the configured allowlist.
    pub fn validate_origin(
        &self,
        origin: Option<&HeaderValue>,
    ) -> Result<Option<HeaderValue>, AuthError> {
        let Some(origin) = origin else {
            return Ok(None);
        };
        let origin_text = origin.to_str().map_err(|_| AuthError::OriginNotAllowed)?;
        let config = self.config().map_err(|_| AuthError::Unavailable)?;
        if config
            .security
            .allowed_origins
            .iter()
            .any(|allowed| allowed == origin_text)
        {
            Ok(Some(origin.clone()))
        } else {
            Err(AuthError::OriginNotAllowed)
        }
    }

    fn config(&self) -> Result<std::sync::RwLockReadGuard<'_, EduMindConfig>, AuthError> {
        self.config.read().map_err(|_| AuthError::Unavailable)
    }
}

fn extract_bearer(headers: &HeaderMap) -> Result<&str, AuthError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .ok_or(AuthError::Required)?
        .to_str()
        .map_err(|_| AuthError::InvalidAuthorizationHeader)?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or(AuthError::InvalidAuthorizationHeader)?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.trim().is_empty() {
        return Err(AuthError::InvalidAuthorizationHeader);
    }
    Ok(token.trim())
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    sub: String,
    #[serde(default)]
    role: Role,
    #[serde(rename = "exp")]
    _exp: usize,
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use serde_json::json;

    use super::{AuthService, Role};
    use crate::config::{EduMindConfig, types::AuthMode};

    #[test]
    fn token_mode_accepts_only_the_configured_token() {
        let mut config = EduMindConfig::default();
        config.gateway.auth.mode = AuthMode::Token;
        config.gateway.auth.token = Some("expected-token".to_owned());
        let auth = AuthService::new(Arc::new(RwLock::new(config)));

        assert_eq!(
            auth.authenticate(Some("expected-token")).unwrap().role,
            Role::Admin
        );
        assert!(auth.authenticate(Some("incorrect-token")).is_err());
    }

    #[test]
    fn jwt_mode_decodes_valid_hs256_claims() {
        let secret = "jwt-secret";
        let token = encode(
            &Header::new(Algorithm::HS256),
            &json!({"sub": "student-42", "role": "admin", "exp": 4_102_444_800_u64}),
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        let mut config = EduMindConfig::default();
        config.gateway.auth.mode = AuthMode::Jwt;
        config.gateway.auth.jwt_secret = Some(secret.to_owned());
        let auth = AuthService::new(Arc::new(RwLock::new(config)));

        let principal = auth.authenticate(Some(&token)).unwrap();

        assert_eq!(principal.subject, "student-42");
        assert_eq!(principal.role, Role::Admin);
    }
}
