mod spotify;
mod state;
mod templates;

use crate::spotify::SpotifyClient;
use crate::state::{get_client_id, AppState};
use crate::templates::{into_response, IndexTemplate, PlayingModel};
use actix_files as fs;
use actix_session::storage::CookieSessionStore;
use actix_session::{Session, SessionMiddleware};
use actix_web::cookie::Key;
use actix_web::{get, web, App, HttpResponse, HttpServer};
use env_logger::Env;
use std::env;
use templates::ConnectTemplate;

#[get("/")]
async fn index(
    session: Session,
    state: web::Data<AppState>,
    spotify_client: web::Data<SpotifyClient>,
) -> HttpResponse {
    let client_id = get_client_id(&session);

    if let Some(_) = state.get_access(&client_id) {
        connected_index(&client_id, state, spotify_client).await
    } else {
        into_response(ConnectTemplate {})
    }
}

async fn connected_index(
    user_id: &str,
    state: web::Data<AppState>,
    spotify_client: web::Data<SpotifyClient>,
) -> HttpResponse {
    if let Some(currently_playing) = spotify_client.get_current_track(user_id, state).await {
        into_response(IndexTemplate {
            player_status: PlayingModel::from(currently_playing),
        })
    } else {
        into_response(IndexTemplate {
            player_status: PlayingModel::new(),
        })
    }
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
