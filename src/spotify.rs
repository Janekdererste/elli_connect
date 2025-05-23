use crate::state::{AppState, SpotifyAppCredentials};
use crate::templates::{into_response, CallbackTemplate};
use actix_session::Session;
use actix_web::{get, web, HttpResponse, Responder, Scope};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use rand::distributions::{Alphanumeric, DistString};
use serde::Deserialize;
use url::Url;

const SPOTIFY_SCOPE: &str = "user-read-private";
const SPOTIFY_AUTH_URL: &str = "https://accounts.spotify.com/authorize";
const SPOTIFY_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const REDIRECT_URI: &str = "http://127.0.0.1:3000/spotify/callback";

pub fn scope() -> Scope {
    web::scope("/spotify")
        .service(authenticate)
        .service(callback)
}

#[get("/auth")]
async fn authenticate(session: Session, app_state: web::Data<AppState>) -> impl Responder {
    let state = rnd_state();
    // take care of error handling later
    session
        .insert("state", &state)
        .expect("Could not store state into session");

    let mut url = Url::parse(SPOTIFY_AUTH_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", app_state.get_spotify_credentials().id())
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

    let token_response = obtain_token(&params.code, state.get_spotify_credentials())
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

async fn obtain_token(
    code: &str,
    spotify_credentials: &SpotifyAppCredentials,
) -> Result<TokenResponse, reqwest::Error> {
    let client = reqwest::Client::new();
    let auth_header = auth_header(spotify_credentials);

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

fn auth_header(spotify_credentials: &SpotifyAppCredentials) -> String {
    let credentials = format!(
        "{}:{}",
        spotify_credentials.id(),
        spotify_credentials.secret()
    );
    format!("Basic {}", BASE64_STANDARD.encode(&credentials))
}
