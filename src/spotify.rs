use crate::state::AppState;
use crate::templates::{into_response, CallbackTemplate};
use actix_session::Session;
use actix_web::{get, web, HttpResponse, Responder, Scope};
use askama::filters::format;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use rand::distributions::{Alphanumeric, DistString};
use reqwest::{Error, Response};
use serde::Deserialize;
use url::Url;

const SPOTIFY_CLIENT_ID: &str = "38f14e6cbed74638857280d0165bc93a";
const SPOTIFY_CLIENT_SECRET: &str = "9854dfcb771b4a7381c276b72d0f3a04";
const SPOTIFY_SCOPE: &str = "user-read-private";
const SPOTIFY_AUTH_URL: &str = "https://accounts.spotify.com/authorize";
const SPOTIFY_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const REDIRECT_URI: &str = "http://127.0.0.1:3000/spotify/callback";

struct SpotifyAppCredentials {
    client_id: String,
    client_secret: String,
}

impl SpotifyAppCredentials {
    fn new(client_secret: &str) -> Self {
        Self {
            client_id: SPOTIFY_CLIENT_ID.to_string(),
            client_secret: client_secret.to_string(),
        }
    }
}

pub fn scope() -> Scope {
    web::scope("/spotify")
        .service(authenticate)
        .service(callback)
}

#[get("/auth")]
async fn authenticate(session: Session) -> impl Responder {
    let state = rnd_state();
    // take care of error handling later
    session
        .insert("state", &state)
        .expect("Could not store state into session");

    let mut url = Url::parse(SPOTIFY_AUTH_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", SPOTIFY_CLIENT_ID)
        .append_pair("scope", SPOTIFY_SCOPE)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("state", &state);

    HttpResponse::Found()
        .append_header(("Location", url.as_str()))
        .finish()
}

#[derive(Deserialize)]
struct CallbackParams {
    code: String,
    state: String,
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    pub expires_in: i32,
    pub refresh_token: String,
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

    let token_response = obtain_token(&params.code)
        .await
        .expect("Could not obtain access token");

    // now, store the client id in the session
    let client_id = get_client_id(&session);

    let template = CallbackTemplate {
        scope: token_response.scope.clone(),
    };

    // store the client id and the token response in the state
    state.store_token_response(&client_id, token_response);

    into_response(template)
}

fn rnd_state() -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), 32)
}

fn get_client_id(session: &Session) -> String {
    if let Some(client_id) = session.get::<String>("client_id").expect("Session Error") {
        client_id
    } else {
        let client_id = rnd_state();
        session
            .insert("client_id", &client_id)
            .expect("Could not store client_id into session");
        client_id
    }
}

async fn obtain_token(code: &str) -> Result<TokenResponse, reqwest::Error> {
    let client = reqwest::Client::new();
    let auth_header = auth_header();

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

fn auth_header() -> String {
    let credentials = format!("{}:{}", SPOTIFY_CLIENT_ID, SPOTIFY_CLIENT_SECRET);
    format!("Basic {}", BASE64_STANDARD.encode(&credentials))
}
