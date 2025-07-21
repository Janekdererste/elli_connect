use crate::spotify::CurrentlyPlaying;
use actix_web::HttpResponse;
use askama::Template;

// Template definitions
#[derive(Template)]
#[template(path = "connect.html")]
pub struct ConnectTemplate {
    pub(crate) b_code: String,
    pub(crate) d_code: String,
}

// Template definitions
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {}

#[derive(Template)]
#[template(path = "device.html")]
pub struct ConnectedDeviceTemplate {
    pub(crate) ccc: String,
}

#[derive(Template)]
#[template(path = "connected.html")]
pub struct ConnectedTemplate {
    pub(crate) player_status: PlayingModel,
    pub(crate) matrix_model: ColorMatrixModel,
}

#[derive(Template)]
#[template(path = "notrack.html")]
pub struct NoTrackTemplate {
    pub(crate) ccc: String,
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorTemplate {
    pub error: String,
    pub description: String,
}

pub fn into_response<T: Template>(template: T) -> HttpResponse {
    match template.render() {
        Ok(rendered) => HttpResponse::Ok().body(rendered),
        Err(_) => HttpResponse::InternalServerError()
            .body("<h1>Error</h1><p>Uh oh, an error while rendering a template</p>"),
    }
}

pub struct ColorMatrixModel {
    pub size: u32,
    pub colors: Vec<String>, // Flattened row-major hex color strings
}

pub struct PlayingModel {
    // is_playing: bool,
    // progress_ms: u64,
    // currently_playing_type: String,
    name: String,
    artists: Vec<String>,
    // album: String,
    pub image_url: String,
}

impl From<CurrentlyPlaying> for PlayingModel {
    fn from(value: CurrentlyPlaying) -> Self {
        if let Some(track) = value.item {
            let artists = track.artists.into_iter().map(|a| a.name).collect();
            let image_url = track
                .album
                .images
                .into_iter()
                .max_by(|a, b| a.width.cmp(&b.width))
                .unwrap_or_default()
                .url;
            Self {
                // is_playing: value.is_playing,
                // progress_ms: value.progress_ms,
                // currently_playing_type: value.currently_playing_type,
                name: track.name,
                artists,
                image_url,
            }
        } else {
            Self {
                // is_playing: value.is_playing,
                // progress_ms: value.progress_ms,
                // currently_playing_type: value.currently_playing_type.clone(),
                name: value.currently_playing_type.to_string(),
                artists: vec!["No data available for currently playing media".to_string()],
                image_url: "https://elemonlabs.com/wp-content/uploads/2020/08/logo_transparent.png"
                    .to_string(),
            }
        }
    }
}
