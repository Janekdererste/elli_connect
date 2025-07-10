use crate::spotify::CurrentlyPlaying;
use actix_web::HttpResponse;
use askama::Template;

// Template definitions
#[derive(Template)]
#[template(path = "connect.html")]
pub struct ConnectTemplate {}

// Template definitions
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub(crate) player_status: PlayingModel,
}

#[derive(Template)]
#[template(path = "callback.html")]
pub struct CallbackTemplate {
    pub scope: String,
}

pub fn into_response<T: Template>(template: T) -> HttpResponse {
    match template.render() {
        Ok(rendered) => HttpResponse::Ok().body(rendered),
        Err(_) => HttpResponse::InternalServerError()
            .body("<h1>Error</h1><p>Uh oh, an error while rendering a template</p>"),
    }
}

pub struct PlayingModel {
    is_playing: bool,
    progress_ms: u64,
    currently_playing_type: String,
    name: String,
    artists: Vec<String>,
    album: String,
    image_url: String,
}

impl PlayingModel {
    pub fn new() -> Self {
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
            image_url,
        }
    }
}
