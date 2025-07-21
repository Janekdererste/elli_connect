mod elli;
mod spotify;
mod state;
mod templates;
mod update;

use crate::elli::ElliConfig;
use crate::spotify::SpotifyClient;
use crate::state::AppState;
use crate::templates::{
    into_response, ColorMatrixModel, ConnectedDeviceTemplate, ConnectedTemplate, IndexTemplate,
    NoTrackTemplate, PlayingModel,
};
use crate::update::ElliUpdate;
use actix_files as fs;
use actix_session::storage::CookieSessionStore;
use actix_session::{Session, SessionMiddleware};
use actix_web::cookie::{Key, SameSite};
use actix_web::error::ErrorInternalServerError;
use actix_web::{get, web, App, HttpResponse, HttpServer};
use env_logger::Env;
use image::imageops::FilterType;
use image::GenericImageView;
use log::info;
use std::env;

#[get("/")]
async fn index() -> Result<HttpResponse, actix_web::Error> {
    Ok(into_response(IndexTemplate {}))
}

#[get("/device/{ccc}")]
async fn device(
    ccc: web::Path<String>,
    session: Session,
) -> Result<HttpResponse, actix_web::Error> {
    // TODO this should first check whether we can parse the device code and return the index with an error if not.

    session
        .insert("ccc", ccc.as_str().to_string())
        .map_err(ErrorInternalServerError)?;
    Ok(into_response(ConnectedDeviceTemplate {
        ccc: ccc.as_str().to_string(),
    }))
}

#[get("/device/{ccc}/connected")]
async fn connected(
    ccc: web::Path<String>,
    app_state: web::Data<AppState>,
    spotify_client: web::Data<SpotifyClient>,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Route: /device/{ccc}/connected");

    // redirect to the device page if not connected
    if let None = app_state.get_access(ccc.as_str()) {
        let response = HttpResponse::Found()
            .append_header(("Location", format!("/device/{ccc}")))
            .finish();
        return Ok(response);
    }

    let update = ElliUpdate::new(ccc.clone(), app_state.clone(), spotify_client.clone()).await?;
    app_state.insert_elli_update(&ccc, update);

    let config = ElliConfig::from_ccc(&ccc)?;
    let elli_size = config.size;

    // fetch currently playing status from spotify
    let playing_model = if let Some(current_track) = spotify_client
        .get_current_track(ccc.as_str(), app_state)
        .await
        .map_err(ErrorInternalServerError)?
    {
        PlayingModel::from(current_track)
    } else {
        let response = into_response(NoTrackTemplate {
            ccc: ccc.as_str().to_string(),
        });
        return Ok(response);
    };

    // if something is playing, fetch the album art
    let image = spotify_client.get_image(&playing_model.image_url).await?;
    let filter_type = if elli_size < 10 {
        FilterType::Nearest
    } else {
        FilterType::Lanczos3
    };

    let downsized_image = image.resize(elli_size, elli_size, filter_type);
    let colors = downsized_image
        .pixels()
        .map(|(_, _, rgba)| format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2]))
        .collect();

    let template = ConnectedTemplate {
        player_status: playing_model,
        matrix_model: ColorMatrixModel {
            size: elli_size,
            colors,
        },
    };
    Ok(into_response(template))
}

#[get("/device/{ccc}/disconnect")]
async fn disconnect(
    ccc: web::Path<String>,
    app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    // remove state from the app state.
    if app_state.has_update(&ccc) {
        if let Some(update) = app_state.remove_elli_update(&ccc) {
            update.close().await?;
        }
    }
    app_state.remove_access(ccc.as_str());

    info!("Disconnect called for ccc: {}", ccc);
    let response = HttpResponse::Found()
        .append_header(("Location", format!("/device/{ccc}")))
        .finish();
    Ok(response)
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
        let session =
            SessionMiddleware::builder(CookieSessionStore::default(), session_key.clone())
                .cookie_http_only(true) // no JavaScript access
                .cookie_same_site(SameSite::Lax) // we want the cookie to be sent from oauth flows
                .cookie_secure(true) // only https or local address
                .cookie_name(String::from("elli-connect"))
                .cookie_path(String::from("/"))
                .build();

        App::new()
            .app_data(state.clone())
            .app_data(spotify_client.clone())
            .wrap(session)
            .service(index)
            .service(spotify::scope())
            .service(device)
            .service(connected)
            .service(disconnect)
            .service(fs::Files::new("/static", "./static").show_files_listing())
    })
    .bind(("127.0.0.1", 3000))?
    .run()
    .await
}
