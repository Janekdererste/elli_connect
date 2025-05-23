use crate::spotify::SpotifyAccess;
use actix_session::Session;
use rand::distributions::{Alphanumeric, DistString};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct AppState {
    spotify_user_access: RwLock<HashMap<String, Arc<SpotifyAccess>>>,
    spotify_credentials: SpotifyAppCredentials,
}

impl AppState {
    // deliberately move the secret.
    pub fn new(spotify_secret: String) -> Self {
        AppState {
            spotify_user_access: RwLock::new(HashMap::new()),
            spotify_credentials: SpotifyAppCredentials::new(spotify_secret),
        }
    }

    pub fn insert_access(&self, client_id: &str, access: SpotifyAccess) {
        let mut tokens = self.spotify_user_access.write().unwrap();
        tokens.insert(client_id.to_string(), Arc::new(access));
    }

    pub fn get_access(&self, client_id: &str) -> Option<Arc<SpotifyAccess>> {
        let tokens = self.spotify_user_access.read().unwrap();
        if let Some(access) = tokens.get(client_id) {
            Some(access.clone())
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

pub fn get_client_id(session: &Session) -> String {
    if let Some(client_id) = session.get::<String>("client_id").expect("Session Error") {
        client_id
    } else {
        let client_id = rnd_string();
        session
            .insert("client_id", &client_id)
            .expect("Could not store client_id into session");
        client_id
    }
}

pub fn rnd_string() -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), 32)
}
