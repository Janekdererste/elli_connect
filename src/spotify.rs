use crate::state::{get_client_id, rnd_string, AppState, SpotifyAppCredentials};
use actix_session::Session;
use actix_web::{get, web, HttpResponse, Responder, Scope};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use log::{info, log};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::time::{Duration, Instant};
use url::Url;

const SPOTIFY_SCOPE: &str = "user-read-currently-playing";
const SPOTIFY_AUTH_URL: &str = "https://accounts.spotify.com/authorize";
const SPOTIFY_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const REDIRECT_URI: &str = "http://127.0.0.1:3000/spotify/callback";

#[derive(Deserialize)]
struct CallbackParams {
    code: String,
    state: String,
}

#[derive(Deserialize, Debug)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct CurrentlyPlaying {
    pub timestamp: u64,
    pub progress_ms: u64,
    pub is_playing: bool,
    pub item: Option<Track>,
    pub currently_playing_type: String,
}

#[derive(Deserialize, Debug)]
pub struct Track {
    pub album: Album,
    pub artists: Vec<Artist>,
    pub duration_ms: u64,
    pub href: String,
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct Album {
    pub album_type: String,
    pub href: String,
    pub id: String,
    pub images: Vec<Image>,
    pub name: String,
    #[serde(rename = "type")]
    pub album_type_detail: String,
    pub uri: String,
}

#[derive(Deserialize, Debug)]
pub struct Artist {
    pub href: String,
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub artist_type: String,
    pub uri: String,
}

#[derive(Deserialize, Debug)]
pub struct Image {
    pub url: String,
    pub height: u32,
    pub width: u32,
}

impl Default for Image {
    fn default() -> Self {
        Image {
            url: String::new(),
            width: 0,
            height: 0,
        }
    }
}

#[derive(Clone)]
pub struct SpotifyClient {
    client: Client,
}

impl SpotifyClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[derive(Debug)]
pub struct SpotifyAccess {
    access_token: String,
    refresh_token: String,
    expires_at: Instant,
}

impl SpotifyAccess {
    pub fn new(access_token: String, refresh_token: String, expires_in: u64) -> Self {
        Self {
            access_token,
            refresh_token,
            expires_at: Self::calculate_expiry(expires_in),
        }
    }

    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }

    pub fn should_refresh(&self) -> bool {
        Instant::now() > self.expires_at
    }

    pub async fn refresh(
        spotify_access: &SpotifyAccess,
        spotify_credentials: &SpotifyAppCredentials,
    ) -> Result<Self, reqwest::Error> {
        let form_data = [
            ("grant_type", "refresh_token"),
            ("refresh_token", spotify_access.refresh_token()),
        ];

        info!(
            "Spotify::refresh: Requesting new access token with refresh token {}",
            spotify_access.refresh_token
        );
        let result = Self::token(&form_data, spotify_credentials).await?;
        let refresh_token = result
            .refresh_token
            .unwrap_or_else(|| spotify_access.refresh_token.clone());
        let new_access = SpotifyAccess::new(result.access_token, refresh_token, result.expires_in);
        Ok(new_access)
    }

    async fn authorize(
        code: &str,
        spotify_app_credentials: &SpotifyAppCredentials,
    ) -> Result<Self, reqwest::Error> {
        let form_data = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
        ];
        info!("Token request from authorize");
        let result = Self::token(&form_data, spotify_app_credentials).await?;

        let access = SpotifyAccess::new(
            result.access_token,
            result.refresh_token.unwrap(),
            result.expires_in,
        );
        Ok(access)
    }

    async fn token<T: Serialize + ?Sized + Debug>(
        form_data: &T,
        spotify_credentials: &SpotifyAppCredentials,
    ) -> Result<TokenResponse, reqwest::Error> {
        let auth_header = auth_header(spotify_credentials);

        let token_response = reqwest::Client::new()
            .post(SPOTIFY_TOKEN_URL)
            .header("Authorization", auth_header)
            .form(form_data)
            .send()
            .await?
            .text()
            .await?;

        info!("Token response: {:#?}", token_response);

        let parsed_response = serde_json::from_str::<TokenResponse>(&token_response).expect(
            "Could not deserialize token response. \
                 Please check if the Spotify API has changed.",
        );

        //.json::<TokenResponse>()
        //.await?;

        Ok(parsed_response)
    }

    fn calculate_expiry(expires_in: u64) -> Instant {
        // stores access and refresh token as well as the instant two minutes before the
        // access_token expires
        Instant::now() + Duration::from_secs(expires_in - 120)
    }
}

pub fn scope() -> Scope {
    web::scope("/spotify")
        .service(authenticate)
        .service(callback)
}

#[get("/auth")]
async fn authenticate(session: Session, app_state: web::Data<AppState>) -> impl Responder {
    let state = rnd_string();
    // take care of error handling later
    session
        .insert("state", &state)
        .expect("Could not store state into session");

    let mut url = Url::parse(SPOTIFY_AUTH_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", app_state.get_spotify_credentials().id())
        .append_pair("scope", SPOTIFY_SCOPE)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("state", &state);

    HttpResponse::Found()
        .append_header(("Location", url.as_str()))
        .finish()
}

#[get("/callback")]
async fn callback(
    params: web::Query<CallbackParams>,
    session: Session,
    state: web::Data<AppState>,
) -> impl Responder {
    // make sure the state matches what we have sent
    if let Some(state) = session
        .get::<String>("state")
        .expect("Could not get state from session")
    {
        if state != params.state {
            return HttpResponse::BadRequest().body("State mismatch");
        } else {
            println!("State matches");
            session
                .remove("state")
                .expect("Could not remove state from session");
        }
    }

    let access = SpotifyAccess::authorize(&params.code, state.get_spotify_credentials())
        .await
        .expect("Could not authorize with Spotify");

    // get the internal client id
    let client_id = get_client_id(&session);

    // store the client id and the token response in the state
    state.insert_access(&client_id, access);

    HttpResponse::Found()
        .append_header(("Location", "/"))
        .finish()
}

async fn obtain_token(
    code: &str,
    spotify_credentials: &SpotifyAppCredentials,
) -> Result<TokenResponse, reqwest::Error> {
    let client = reqwest::Client::new();
    let auth_header = auth_header(spotify_credentials);

    let form_data = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
    ];

    let token_response = client
        .post(SPOTIFY_TOKEN_URL)
        .header("Authorization", auth_header)
        .form(&form_data)
        .send()
        .await?
        .json::<TokenResponse>()
        .await?;

    Ok(token_response)
}

async fn get_current_track() -> () {}

fn auth_header(spotify_credentials: &SpotifyAppCredentials) -> String {
    let credentials = format!(
        "{}:{}",
        spotify_credentials.id(),
        spotify_credentials.secret()
    );
    format!("Basic {}", BASE64_STANDARD.encode(&credentials))
}
