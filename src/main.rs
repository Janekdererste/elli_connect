mod elli;
mod spotify;
mod state;
mod templates;

use crate::elli::ElliConfig;
use crate::spotify::SpotifyClient;
use crate::state::{get_session_id, AppState};
use crate::templates::{
    into_response, ColorMatrixModel, ConnectedDeviceTemplate, ConnectedTemplate, ErrorTemplate,
    IndexTemplate, PlayingModel,
};
use actix_files as fs;
use actix_session::storage::CookieSessionStore;
use actix_session::{Session, SessionMiddleware};
use actix_web::cookie::Key;
use actix_web::error::{ContentTypeError, ErrorInternalServerError};
use actix_web::http::StatusCode;
use actix_web::{get, web, App, HttpResponse, HttpServer};
use askama::Template;
use env_logger::Env;
use image::imageops::FilterType;
use image::{GenericImageView, Pixel};
use log::info;
use std::env;
use templates::ConnectTemplate;

#[get("/")]
async fn index() -> Result<HttpResponse, actix_web::Error> {
    Ok(into_response(IndexTemplate {}))
}

#[get("/{ccc}")]
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

#[get("/{ccc}/connected")]
async fn connected(
    ccc: web::Path<String>,
    state: web::Data<AppState>,
    spotify_client: web::Data<SpotifyClient>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Some(current_track) = spotify_client
        .get_current_track(ccc.as_str(), state)
        .await
        .map_err(ErrorInternalServerError)?
    {
        let model = PlayingModel::from(current_track);
        Ok(into_response(ConnectedTemplate {
            player_status: model,
        }))
        //let image = spotify_client.get_image(&model.image_url).await?;
    } else {
        Ok(HttpResponse::InternalServerError()
            .body("Something went wrong. Please try again later."))
    }
}

// async fn index(
//     session: Session,
//     state: web::Data<AppState>,
//     spotify_client: web::Data<SpotifyClient>,
// ) -> Result<HttpResponse, actix_web::Error> {
//     let session_id = get_session_id(&session).map_err(ErrorInternalServerError)?;
//
//     if let Some(_) = state.get_access(&session_id) {
//         Ok(connected_index(&session_id, state, spotify_client).await?)
//     } else {
//         Ok(into_response(ConnectTemplate {
//             b_code: String::new(),
//             d_code: String::new(),
//         }))
//     }
// }

// async fn device(
//     ccc: web::Path<String>,
//     session: Session,
//     state: web::Data<AppState>,
// ) -> Result<HttpResponse, actix_web::Error> {
//     match ElliConfig::from_ccc(&ccc) {
//         Ok(config) => {
//             // first off store the ccc parameter in the session for later
//             session
//                 .insert("ccc", ccc.as_str().to_string())
//                 .map_err(ErrorInternalServerError)?;
//             let b_code = config.b_code.clone();
//             let d_code = config.d_code.clone();
//
//             if let None = state.get_elli_state(&ccc) {
//                 state.insert_elli_config(config);
//             }
//             // use unwrap here, as we have just established that the state is present.
//             let elli_state = state.get_elli_state(&ccc).unwrap();
//
//             if let Some(_) = &elli_state.connected_spotify_account {
//                 let redirect_url = format!("/{}/connected", ccc.as_str());
//                 let response = HttpResponse::Found()
//                     .append_header(("Location", redirect_url))
//                     .finish();
//                 Ok(response)
//             } else {
//                 Ok(into_response(ConnectTemplate { b_code, d_code }))
//             }
//         }
//         Err(_) => create_error_response(
//             "Invalid device Code",
//             format!("{:?} could not be parsed", ccc).as_str(),
//         ),
//     }
// }
// #[get("/{ccc}/connected")]
// async fn connected(
//     ccc: web::Path<String>,
//     state: web::Data<AppState>,
//     spotify_client: web::Data<SpotifyClient>,
// ) -> Result<HttpResponse, actix_web::Error> {
//     if let Some(elli_state) = state.get_elli_state(&ccc) {
//         connected_index(&ccc, state, spotify_client).await
//     } else {
//         Ok(HttpResponse::Ok().body("no elli state so far."))
//     }
// }

fn create_error_response(error: &str, description: &str) -> Result<HttpResponse, actix_web::Error> {
    let template = ErrorTemplate {
        error: String::from(error),
        description: String::from(description),
    };
    let rendered = template.render().map_err(ErrorInternalServerError)?;
    let response = HttpResponse::BadRequest()
        .content_type("text/html")
        .body(rendered);
    Ok(response)
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
            //player_status: model,
            //color_matrix: matrix_model,
        }))
    } else {
        Ok(into_response(IndexTemplate {
            //player_status: PlayingModel::new(),
            //color_matrix: ColorMatrixModel::default(),
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
            .service(device)
            .service(fs::Files::new("/static", "./static").show_files_listing())
    })
    .bind(("127.0.0.1", 3000))?
    .run()
    .await
}
