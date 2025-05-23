mod spotify;
mod state;
mod templates;

use crate::spotify::{CurrentlyPlaying, SpotifyAccess};
use crate::state::{get_client_id, AppState};
use crate::templates::{into_response, IndexTemplate};
use actix_files as fs;
use actix_session::storage::CookieSessionStore;
use actix_session::{Session, SessionMiddleware};
use actix_web::cookie::Key;
use actix_web::{get, web, App, HttpResponse, HttpServer};
use env_logger::Env;
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
        let credentials = state.get_spotify_credentials();
        let new_access = SpotifyAccess::refresh(&access, credentials).await.unwrap();
        state.insert_access(client_id, new_access);
    }
    state.get_access(client_id).unwrap()
}

async fn connected_index(client_id: &str, state: web::Data<AppState>) -> HttpResponse {
    let access = get_fresh_access(client_id, state).await;

    let bearer = format!("Bearer {}", access.access_token());

    // fetch player state
    let result: CurrentlyPlaying = reqwest::Client::new()
        .get("https://api.spotify.com/v1/me/player/currently-playing")
        .header("Authorization", bearer)
        .send()
        .await
        .expect("Failed to fetch spotify currently playing")
        .json()
        .await
        .expect("Failed to parse spotify response");

    let cover_url = if let Some(album) = &result.item {
        &album.album.images.get(0).unwrap().url
    } else {
        ""
    };

    into_response(IndexTemplate { cover_url })
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Server starting at http://127.0.0.1:3000");

    // Initialize the logger
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    let secret = env::var("SPOTIFY_CLIENT_SECRET").expect("SPOTIFY_CLIENT_SECRET must be set");
    let session_key = Key::generate();
    let state = web::Data::new(AppState::new(secret));

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
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
