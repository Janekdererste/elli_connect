use crate::spotify::TokenResponse;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct AppState {
    tokens: Mutex<HashMap<String, TokenResponse>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            tokens: Mutex::new(HashMap::new()),
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
}
