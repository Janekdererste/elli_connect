mod spotify;
mod state;
mod templates;

use crate::spotify::{CurrentlyPlaying, Image, SpotifyAccess, SpotifyClient, Track};
use crate::state::{get_client_id, AppState};
use crate::templates::{into_response, IndexTemplate};
use actix_files as fs;
use actix_session::storage::CookieSessionStore;
use actix_session::{Session, SessionMiddleware};
use actix_web::cookie::Key;
use actix_web::{get, web, App, HttpResponse, HttpServer};
use env_logger::Env;
use log::{error, info};
use std::env;
use std::sync::Arc;
use templates::ConnectTemplate;

#[get("/")]
async fn index(session: Session, state: web::Data<AppState>) -> HttpResponse {
    let client_id = get_client_id(&session);

    if let Some(_) = state.get_access(&client_id) {
        connected_index(&client_id, state).await
    } else {
        into_response(ConnectTemplate {})
    }
}

async fn get_fresh_access(client_id: &str, state: web::Data<AppState>) -> Arc<SpotifyAccess> {
    let access = state.get_access(client_id).unwrap();
    if access.should_refresh() {
        info!("Must refresh access token.");
        let credentials = state.get_spotify_credentials();
        let new_access = SpotifyAccess::refresh(&access, credentials).await.unwrap();
        state.insert_access(client_id, new_access);
    }
    state.get_access(client_id).unwrap()
}

async fn connected_index(client_id: &str, state: web::Data<AppState>) -> HttpResponse {
    let access = get_fresh_access(client_id, state).await;

    let bearer = format!("Bearer {}", access.access_token());

    info!("Fetching currently playing state from spotify API");
    // fetch player state
    let response = reqwest::Client::new()
        .get("https://api.spotify.com/v1/me/player/currently-playing")
        .header("Authorization", bearer)
        .send()
        .await
        .expect("Failed to fetch currently playing");

    info!("Got response: {response:#?}");

    if (response.status() == reqwest::StatusCode::NO_CONTENT) {
        info!("No track playing");
        return into_response(IndexTemplate {
            playing_status: "No track playing",
            player_status: PlayingModel::new(),
        });
    }
    let response_text = response
        .text()
        .await
        .expect("Failed to parse currently playing");

    match serde_json::from_str::<CurrentlyPlaying>(&response_text) {
        Ok(currently_playing) => {
            info!("Got currently playing: {currently_playing:#?}");
        }
        Err(e) => {
            error!("Failed to parse currently playing: {e:#?}");
        }
    }

    let parsed_response = serde_json::from_str::<CurrentlyPlaying>(&response_text)
        .expect("Failed to parse currently playing");
    let player_status = PlayingModel::from(parsed_response);

    into_response(IndexTemplate {
        playing_status: &response_text,
        player_status,
    })
}

struct PlayingModel {
    is_playing: bool,
    progress_ms: u64,
    currently_playing_type: String,
    name: String,
    artists: Vec<String>,
    album: String,
    image_url: String,
}

impl PlayingModel {
    fn new() -> Self {
        Self {
            is_playing: false,
            progress_ms: 0,
            currently_playing_type: String::new(),
            name: String::new(),
            artists: vec![],
            album: String::new(),
            image_url: String::new(),
        }
    }
}

impl From<CurrentlyPlaying> for PlayingModel {
    fn from(value: CurrentlyPlaying) -> Self {
        let track = value.item.unwrap();
        let artists = track.artists.into_iter().map(|a| a.name).collect();
        let image_url = track
            .album
            .images
            .into_iter()
            .max_by(|a, b| a.width.cmp(&b.width))
            .unwrap_or_default()
            .url;
        Self {
            is_playing: value.is_playing,
            progress_ms: value.progress_ms,
            currently_playing_type: value.currently_playing_type,
            name: track.name,
            artists,
            album: track.album.name,
            image_url: image_url,
        }
    }

    // fn track_details(track: Option<Track>) -> [String, String, String] {
    //
    // }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Server starting at http://127.0.0.1:3000");

    // Initialize the logger
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    let secret = env::var("SPOTIFY_CLIENT_SECRET").expect("SPOTIFY_CLIENT_SECRET must be set");
    let session_key = Key::generate();
    let state = web::Data::new(AppState::new(secret));
    let spotify_client = web::Data::new(SpotifyClient::new());

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .app_data(spotify_client.clone())
            .wrap(SessionMiddleware::new(
                CookieSessionStore::default(),
                session_key.clone(),
            ))
            .service(index)
            .service(spotify::scope())
            .service(fs::Files::new("/static", "./static").show_files_listing())
    })
    .bind(("127.0.0.1", 3000))?
    .run()
    .await
}
