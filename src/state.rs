use crate::elli::ElliConfig;
use crate::spotify::SpotifyAccess;
use actix_session::Session;
use rand::distributions::{Alphanumeric, DistString};
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, RwLock};

pub struct AppState {
    spotify_user_access: RwLock<HashMap<String, Arc<SpotifyAccess>>>,
    elli_states: RwLock<HashMap<String, Arc<ElliState>>>,
    spotify_credentials: SpotifyAppCredentials,
}

impl AppState {
    // deliberately move the secret.
    pub fn new(spotify_secret: String) -> Self {
        AppState {
            spotify_user_access: RwLock::new(HashMap::new()),
            elli_states: RwLock::new(HashMap::new()),
            spotify_credentials: SpotifyAppCredentials::new(spotify_secret),
        }
    }

    pub fn insert_access(&self, client_id: &str, access: SpotifyAccess) {
        // I think unwrap is fine here, as the insert should not panic
        let mut tokens = self.spotify_user_access.write().unwrap();
        tokens.insert(client_id.to_string(), Arc::new(access));
    }

    pub fn get_access(&self, client_id: &str) -> Option<Arc<SpotifyAccess>> {
        // I think unwrap is fine here, as the get should not panic
        let tokens = self.spotify_user_access.read().unwrap();
        if let Some(access) = tokens.get(client_id) {
            Some(access.clone())
        } else {
            None
        }
    }

    pub fn insert_elli_config(&self, config: ElliConfig) {
        let key = format!("{}{}", config.b_code, config.d_code);
        let state = ElliState {
            config,
            connected_spotify_account: None,
        };
        let mut elli_states = self.elli_states.write().unwrap();
        elli_states.insert(key, Arc::new(state));
    }

    pub fn get_elli_state(&self, ccc: &str) -> Option<Arc<ElliState>> {
        let elli_states = self.elli_states.read().unwrap();
        if let Some(state) = elli_states.get(ccc) {
            Some(state.clone())
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

pub fn get_session_id(session: &Session) -> Result<String, Box<dyn Error>> {
    if let Some(session_id) = session.get::<String>("session_id")? {
        Ok(session_id)
    } else {
        let session_id = rnd_string();
        session.insert("session_id", &session_id)?;
        Ok(session_id)
    }
}

pub struct ElliState {
    config: ElliConfig,
    pub(crate) connected_spotify_account: Option<String>, // use spotify id for now.
}

pub fn rnd_string() -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), 32)
}
