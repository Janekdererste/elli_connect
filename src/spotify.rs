use crate::state::{rnd_string, AppState, SpotifyAppCredentials};
use actix_session::Session;
use actix_web::error::ErrorInternalServerError;
use actix_web::{get, web, HttpResponse, Scope};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use image::DynamicImage;
use log::info;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::sync::Arc;
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
    pub expires_in: u64,
    pub refresh_token: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct CurrentlyPlaying {
    // pub progress_ms: u64,
    // pub is_playing: bool,
    pub item: Option<Track>,
    pub currently_playing_type: String,
}

#[derive(Deserialize, Debug)]
pub struct Track {
    pub album: Album,
    pub artists: Vec<Artist>,
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct Album {
    pub images: Vec<Image>,
}

#[derive(Deserialize, Debug)]
pub struct Artist {
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct Image {
    pub url: String,
    pub width: u32,
}

impl Default for Image {
    fn default() -> Self {
        Image {
            url: String::new(),
            width: 0,
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

    pub async fn get_current_track(
        &self,
        ccc: &str,
        state: web::Data<AppState>,
    ) -> Result<Option<CurrentlyPlaying>, Box<dyn std::error::Error>> {
        info!("Fetching current track for ccc: {}", ccc);
        let access = Self::ensure_fresh_token(ccc, state).await?;
        let bearer = format!("Bearer {}", access.access_token());

        let response = self
            .client
            .get("https://api.spotify.com/v1/me/player/currently-playing")
            .header("Authorization", bearer)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            Ok(None)
        } else {
            // let bla = response.text().await?;
            // info!("get track response: {}", bla);
            // let result = serde_json::from_str::<CurrentlyPlaying>(&bla)?;
            let result = response.json::<CurrentlyPlaying>().await?;
            Ok(Some(result))
        }
    }

    pub async fn get_image(
        &self,
        image_url: &str,
    ) -> Result<DynamicImage, Box<dyn std::error::Error>> {
        info!("Fetching image: {}", image_url);
        let response = self.client.get(image_url).send().await?;
        let data = response.bytes().await?;
        let image = image::load_from_memory(&data)?;

        Ok(image)
    }

    async fn ensure_fresh_token(
        ccc: &str,
        state: web::Data<AppState>,
    ) -> Result<Arc<SpotifyAccess>, Box<dyn std::error::Error>> {
        let access = state
            .get_access(ccc)
            .ok_or_else(|| "No access token found, but should be present.")?;
        if access.should_refresh() {
            let spotify_credentials = state.get_spotify_credentials();
            let new_access = SpotifyAccess::refresh(&access, spotify_credentials).await?;
            state.insert_access(ccc, new_access);
        }
        // we use unwrap because we have just inserted the access_token
        let result = state
            .get_access(ccc)
            .ok_or_else(|| "Failed to retreive freshly inserted token")?;
        Ok(result)
    }
}

#[derive(Debug)]
pub struct SpotifyAccess {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Instant,
}

impl SpotifyAccess {
    pub fn new(access_token: String, refresh_token: Option<String>, expires_in: u64) -> Self {
        Self {
            access_token,
            refresh_token,
            expires_at: Self::calculate_expiry(expires_in),
        }
    }

    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    pub fn refresh_token(&self) -> &Option<String> {
        &self.refresh_token
    }

    pub fn should_refresh(&self) -> bool {
        Instant::now() > self.expires_at
    }

    pub async fn refresh(
        spotify_access: &SpotifyAccess,
        spotify_credentials: &SpotifyAppCredentials,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if let Some(refresh_token) = spotify_access.refresh_token() {
            let form_data = [
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh_token),
            ];
            let result = Self::token(&form_data, spotify_credentials).await?;
            let new_refresh_token = result
                .refresh_token
                .unwrap_or_else(|| refresh_token.clone());
            let new_access = SpotifyAccess::new(
                result.access_token,
                Some(new_refresh_token),
                result.expires_in,
            );
            Ok(new_access)
        } else {
            Err("No refresh token")?
        }
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
        let result = Self::token(&form_data, spotify_app_credentials).await?;

        let access =
            SpotifyAccess::new(result.access_token, result.refresh_token, result.expires_in);
        Ok(access)
    }

    async fn token<T: Serialize + ?Sized + Debug>(
        form_data: &T,
        spotify_credentials: &SpotifyAppCredentials,
    ) -> Result<TokenResponse, reqwest::Error> {
        let auth_header = auth_header(spotify_credentials);

        // TODO replace with spotify client
        let token_response = Client::new()
            .post(SPOTIFY_TOKEN_URL)
            .header("Authorization", auth_header)
            .form(form_data)
            .send()
            .await?
            .text()
            .await?;

        let parsed_response = serde_json::from_str::<TokenResponse>(&token_response).expect(
            "Could not deserialize token response. \
                 Please check if the Spotify API has changed.",
        );

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
async fn authenticate(
    session: Session,
    app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    // check if we have stored an elli ccc. If not redirect to index page.
    let ccc = if let Some(ccc) = session
        .get::<String>("ccc")
        .map_err(ErrorInternalServerError)?
    {
        ccc
    } else {
        let response = HttpResponse::Found()
            .append_header(("Location", "/"))
            .finish();
        return Ok(response);
    };

    info!("/auth: Session entries: {:#?}", session.entries());

    // random state to evaluate in the callback
    let state = rnd_string();
    // we can use unwrap here, as we hardcoded this url
    let mut url = Url::parse(SPOTIFY_AUTH_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", app_state.get_spotify_credentials().id())
        .append_pair("scope", SPOTIFY_SCOPE)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("state", &state);

    // store the state in the app_state
    app_state.insert_oauth_state(&ccc, state);

    let response = HttpResponse::Found()
        .append_header(("Location", url.as_str()))
        .finish();
    Ok(response)
}

#[get("/callback")]
async fn callback(
    params: web::Query<CallbackParams>,
    session: Session,
    app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    info!("/callback: Session entries: {:#?}", session.entries());

    // get the session key
    let ccc = if let Some(ccc) = session
        .get::<String>("ccc")
        .map_err(ErrorInternalServerError)?
    {
        ccc
    } else {
        let response = HttpResponse::BadRequest().body("No session id found");
        return Ok(response);
    };

    // check whether the previously saved state matches the state param sent back by the auth api
    if let Some(state) = app_state.get_oauth_state(&ccc) {
        if state == params.state {
            app_state.remove_oauth_state(&ccc);
        } else {
            let response = HttpResponse::BadRequest().body("State mismatch");
            return Ok(response);
        }
    } else {
        let response = HttpResponse::BadRequest().body("No state found");
        return Ok(response);
    }

    // switch authorization token against access token and refresh token
    let access = SpotifyAccess::authorize(&params.code, app_state.get_spotify_credentials())
        .await
        .map_err(ErrorInternalServerError)?;

    app_state.insert_access(&ccc, access);
    let redirect_path = format!("/device/{}/connected", ccc);
    let response = HttpResponse::Found()
        .append_header(("Location", redirect_path))
        .finish();
    Ok(response)
}

fn auth_header(spotify_credentials: &SpotifyAppCredentials) -> String {
    let credentials = format!(
        "{}:{}",
        spotify_credentials.id(),
        spotify_credentials.secret()
    );
    format!("Basic {}", BASE64_STANDARD.encode(&credentials))
}
