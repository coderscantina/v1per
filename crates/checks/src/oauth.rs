//! OAuth 2 client credentials tokens, cached across checks until they expire, as Kuma caches
//! them per monitor.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use url::Url;
use url::form_urlencoded::{Serializer, byte_serialize};

use crate::definition::{OAuthClientAuth, Secret};

pub(crate) struct OAuthRequest<'a> {
    pub token_url: &'a Url,
    pub client_id: &'a str,
    pub client_secret: &'a Secret,
    pub scopes: Option<&'a str>,
    pub audience: Option<&'a str>,
    pub client_auth: OAuthClientAuth,
}

#[derive(PartialEq, Eq, Hash)]
struct TokenKey {
    token_url: String,
    client_id: String,
    client_secret: String,
    scopes: Option<String>,
    audience: Option<String>,
}

struct Token {
    authorization: String,
    expires_at: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct OAuthTokens(Mutex<HashMap<TokenKey, Token>>);

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

impl OAuthTokens {
    /// The `Authorization` value for the request. `refresh` skips the cache, as Kuma does once
    /// after a 401. A token without `expires_in` is kept until a 401, like Kuma.
    pub(crate) async fn authorization(
        &self,
        client: &reqwest::Client,
        request: &OAuthRequest<'_>,
        refresh: bool,
    ) -> Result<String, String> {
        let key = TokenKey {
            token_url: request.token_url.to_string(),
            client_id: request.client_id.to_owned(),
            client_secret: request.client_secret.expose().to_owned(),
            scopes: request.scopes.map(str::to_owned),
            audience: request.audience.map(str::to_owned),
        };
        if !refresh {
            let tokens = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(token) = tokens.get(&key)
                && token.expires_at.is_none_or(|at| at > Instant::now())
            {
                return Ok(token.authorization.clone());
            }
        }
        let token = fetch(client, request).await?;
        let authorization = token.authorization.clone();
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, token);
        Ok(authorization)
    }
}

async fn fetch(client: &reqwest::Client, request: &OAuthRequest<'_>) -> Result<Token, String> {
    // The serializer is not Send, so it lives only inside this block and never across an await.
    let body = {
        let mut form = Serializer::new(String::new());
        form.append_pair("grant_type", "client_credentials");
        if let Some(scope) = request.scopes.filter(|scope| !scope.is_empty()) {
            form.append_pair("scope", scope);
        }
        if let Some(audience) = request.audience.filter(|audience| !audience.is_empty()) {
            form.append_pair("audience", audience);
        }
        if let OAuthClientAuth::ClientSecretPost = request.client_auth {
            form.append_pair("client_id", request.client_id);
            form.append_pair("client_secret", request.client_secret.expose());
        }
        form.finish()
    };
    let mut builder = client
        .post(request.token_url.clone())
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let OAuthClientAuth::ClientSecretBasic = request.client_auth {
        // RFC 6749 2.3.1: both parts are form-encoded before base64.
        let encode = |value: &str| byte_serialize(value.as_bytes()).collect::<String>();
        let credentials = format!(
            "{}:{}",
            encode(request.client_id),
            encode(request.client_secret.expose())
        );
        builder = builder.header(
            AUTHORIZATION,
            format!("Basic {}", STANDARD.encode(credentials)),
        );
    }
    let requested_at = Instant::now();
    let response = builder
        .body(body)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.bytes().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(match serde_json::from_slice::<TokenError>(&body) {
            Ok(TokenError {
                error,
                error_description: Some(description),
            }) => format!("{error} ({description})"),
            Ok(TokenError { error, .. }) => error,
            Err(_) => format!("expected 200 OK, got: {status}"),
        });
    }
    let token: TokenResponse = serde_json::from_slice(&body)
        .map_err(|error| format!("invalid token response: {error}"))?;
    Ok(Token {
        authorization: format!("{} {}", token.token_type, token.access_token),
        expires_at: token
            .expires_in
            .map(|seconds| requested_at + Duration::from_secs(seconds)),
    })
}
