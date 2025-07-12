mod elli;
mod spotify;
mod state;
mod templates;

use crate::spotify::SpotifyClient;
use crate::state::{get_session_id, AppState};
use crate::templates::{into_response, ColorMatrixModel, IndexTemplate, PlayingModel};
use actix_files as fs;
use actix_session::storage::CookieSessionStore;
use actix_session::{Session, SessionMiddleware};
use actix_web::cookie::Key;
use actix_web::error::ErrorInternalServerError;
use actix_web::{get, web, App, HttpResponse, HttpServer};
use env_logger::Env;
use image::imageops::FilterType;
use image::{GenericImageView, Pixel};
use log::info;
use std::env;
use templates::ConnectTemplate;

#[get("/")]
async fn index(
    session: Session,
    state: web::Data<AppState>,
    spotify_client: web::Data<SpotifyClient>,
) -> Result<HttpResponse, actix_web::Error> {
    let session_id = get_session_id(&session).map_err(ErrorInternalServerError)?;

    if let Some(_) = state.get_access(&session_id) {
        Ok(connected_index(&session_id, state, spotify_client).await?)
    } else {
        Ok(into_response(ConnectTemplate {}))
    }
}

async fn connected_index(
    user_id: &str,
    state: web::Data<AppState>,
    spotify_client: web::Data<SpotifyClient>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Some(currently_playing) = spotify_client
        .get_current_track(user_id, state)
        .await
        .map_err(ErrorInternalServerError)?
    {
        let model = PlayingModel::from(currently_playing);
        let image = spotify_client.get_image(&model.image_url).await?;
        // use fixed sample size for now. This should be taken from the lamp actually.
        let downsized_image = image.resize(5, 5, FilterType::Nearest);
        let rgb_image = downsized_image.to_rgb8();
        let (width, height) = rgb_image.dimensions();

        let colors: Vec<String> = rgb_image
            .pixels()
            .map(|p| format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2]))
            .collect();
        info!("{colors:#?}");
        let matrix_model = ColorMatrixModel {
            width,
            height,
            colors,
        };

        Ok(into_response(IndexTemplate {
            player_status: model,
            color_matrix: matrix_model,
        }))
    } else {
        Ok(into_response(IndexTemplate {
            player_status: PlayingModel::new(),
            color_matrix: ColorMatrixModel::default(),
        }))
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
