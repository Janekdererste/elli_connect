use crate::spotify::TokenResponse;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct AppState {
    tokens: Mutex<HashMap<String, TokenResponse>>,
    spotify_credentials: SpotifyAppCredentials,
}

impl AppState {
    // deliberately move the secret.
    pub fn new(spotify_secret: String) -> Self {
        AppState {
            tokens: Mutex::new(HashMap::new()),
            spotify_credentials: SpotifyAppCredentials::new(spotify_secret),
        }
    }

    pub fn store_token_response(&self, client_id: &str, response: TokenResponse) {
        let mut tokens = self.tokens.lock().unwrap();
        tokens.insert(client_id.to_string(), response);
    }

    pub fn get_access_token(&self, client_id: &str) -> Option<String> {
        let tokens = self.tokens.lock().unwrap();
        if let Some(response) = tokens.get(client_id) {
            Some(response.access_token.clone())
        } else {
            None
        }
    }

    pub fn get_spotify_credentials(&self) -> &SpotifyAppCredentials {
        &self.spotify_credentials
    }
}

pub struct SpotifyAppCredentials {
    client_id: String,
    client_secret: String,
}

impl SpotifyAppCredentials {
    fn new(client_secret: String) -> Self {
        Self {
            client_id: "38f14e6cbed74638857280d0165bc93a".to_string(),
            client_secret,
        }
    }

    pub fn secret(&self) -> &str {
        &self.client_secret
    }

    pub fn id(&self) -> &str {
        &self.client_id
    }
}
