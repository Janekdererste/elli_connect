use crate::elli::{ElliConfig, ElliConnection};
use crate::spotify::SpotifyAccess;
use actix_session::Session;
use rand::distributions::{Alphanumeric, DistString};
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, RwLock};

pub struct AppState {
    spotify_user_access: RwLock<HashMap<String, Arc<SpotifyAccess>>>,
    elli_connections: RwLock<HashMap<String, Arc<ElliConnection>>>,
    spotify_credentials: SpotifyAppCredentials,
    oauth_states: RwLock<HashMap<String, String>>,
}

impl AppState {
    // deliberately move the secret.
    pub fn new(spotify_secret: String) -> Self {
        AppState {
            spotify_user_access: RwLock::new(HashMap::new()),
            elli_connections: RwLock::new(HashMap::new()),
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

    pub fn insert_elli_connection(&self, key: &str, connection: ElliConnection) {
        let mut elli_connections = self.elli_connections.write().unwrap();
        elli_connections.insert(key.to_string(), Arc::new(connection));
    }

    pub fn get_elli_connection(&self, key: &str) -> Option<Arc<ElliConnection>> {
        let elli_connections = self.elli_connections.read().unwrap();
        if let Some(connection) = elli_connections.get(key) {
            Some(connection.clone())
        } else {
            None
        }
    }

    pub fn remove_elli_connection(&self, key: &str) -> Option<Arc<ElliConnection>> {
        // TODO the connection must be stopped too.
        let mut elli_connections = self.elli_connections.write().unwrap();
        elli_connections.remove(key)
    }

    // pub fn insert_elli_config(&self, config: ElliConfig) {
    //     let key = format!("{}{}", config.b_code, config.d_code);
    //     let state = ElliState {
    //         config,
    //         connected_spotify_account: None,
    //     };
    //     let mut elli_states = self.elli_states.write().unwrap();
    //     elli_states.insert(key, Arc::new(state));
    // }
    //
    // pub fn get_elli_state(&self, ccc: &str) -> Option<Arc<ElliState>> {
    //     let elli_states = self.elli_states.read().unwrap();
    //     if let Some(state) = elli_states.get(ccc) {
    //         Some(state.clone())
    //     } else {
    //         None
    //     }
    // }

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
