use crate::spotify::SpotifyAccess;
use rand::distributions::{Alphanumeric, DistString};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct AppState {
    spotify_user_access: RwLock<HashMap<String, Arc<SpotifyAccess>>>,
    spotify_credentials: SpotifyAppCredentials,
    oauth_states: RwLock<HashMap<String, String>>,
}

impl AppState {
    // deliberately move the secret.
    pub fn new(spotify_secret: String) -> Self {
        AppState {
            spotify_user_access: RwLock::new(HashMap::new()),
            oauth_states: RwLock::new(HashMap::new()),
            spotify_credentials: SpotifyAppCredentials::new(spotify_secret),
        }
    }

    pub fn insert_access(&self, key: &str, access: SpotifyAccess) {
        // I think unwrap is fine here, as the insert should not panic
        let mut tokens = self.spotify_user_access.write().unwrap();
        tokens.insert(key.to_string(), Arc::new(access));
    }

    pub fn get_access(&self, key: &str) -> Option<Arc<SpotifyAccess>> {
        // I think unwrap is fine here, as the get should not panic
        let tokens = self.spotify_user_access.read().unwrap();
        if let Some(access) = tokens.get(key) {
            Some(access.clone())
        } else {
            None
        }
    }

    pub fn remove_access(&self, key: &str) {
        let mut tokens = self.spotify_user_access.write().unwrap();
        tokens.remove(key);
    }

    pub fn get_all_devices(&self) -> Vec<String> {
        self.spotify_user_access
            .read()
            .unwrap()
            .keys()
            .map(|ccc| ccc.clone())
            .collect()
    }

    pub fn get_spotify_credentials(&self) -> &SpotifyAppCredentials {
        &self.spotify_credentials
    }

    pub fn insert_oauth_state(&self, key: &str, state: String) {
        let mut oauth_states = self.oauth_states.write().unwrap();
        oauth_states.insert(key.to_string(), state);
    }

    pub fn get_oauth_state(&self, key: &str) -> Option<String> {
        let oauth_states = self.oauth_states.read().unwrap();
        if let Some(state) = oauth_states.get(key) {
            Some(state.clone())
        } else {
            None
        }
    }

    pub fn remove_oauth_state(&self, key: &str) {
        let mut oauth_states = self.oauth_states.write().unwrap();
        oauth_states.remove(key);
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

pub fn rnd_string() -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), 32)
}
