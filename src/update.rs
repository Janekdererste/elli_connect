use crate::elli::elli_connection::ElliConnection;
use crate::elli::messages::websocket::PixelData;
use crate::elli::ElliConfig;
use crate::spotify::SpotifyClient;
use crate::state::AppState;
use crate::templates::PlayingModel;
use actix_web::error::ErrorInternalServerError;
use actix_web::web;
use image::imageops::FilterType;
use image::GenericImageView;
use log::info;
use std::error::Error;
use std::option::Option;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tokio::time::interval;

pub struct ElliUpdate {
    close_tx: oneshot::Sender<()>,
    task_handle: JoinHandle<()>,
    last_image_url: Arc<RwLock<String>>,
}

impl ElliUpdate {
    pub async fn new(
        ccc: String,
        app_state: web::Data<AppState>,
        spotify_client: web::Data<SpotifyClient>,
    ) -> Result<Self, Box<dyn Error>> {
        let (close_tx, close_rx) = oneshot::channel();
        let last_image_url = Arc::new(RwLock::new(String::new()));
        let handle = Self::start_update(
            ccc,
            last_image_url.clone(),
            app_state,
            spotify_client,
            close_rx,
        )
        .await?;
        let update = Self {
            close_tx: close_tx,
            task_handle: handle,
            last_image_url,
        };
        Ok(update)
    }

    pub async fn close(self) -> Result<(), Box<dyn Error>> {
        let _ = self.close_tx.send(());
        self.task_handle.await?;

        Ok(())
    }

    async fn start_update(
        ccc: String,
        last_image_url: Arc<RwLock<String>>,
        app_state: web::Data<AppState>,
        spotify_client: web::Data<SpotifyClient>,
        mut rx_close: oneshot::Receiver<()>,
    ) -> Result<JoinHandle<()>, Box<dyn Error>> {
        let config = ElliConfig::from_ccc(&ccc)?;
        let handle = tokio::spawn(async move {
            let i = config.size * config.size / 2;
            let mut update_interval = interval(Duration::from_secs(i as u64));
            info!("Starting update worker for {} with interval {}s", ccc, i);
            loop {
                tokio::select! {
                    _ = &mut rx_close => {
                        info!("received stop update signal for {}", ccc);
                        break;
                    }
                    _ = update_interval.tick() => {
                        info!("updating {}", ccc);
                        do_update(ccc.clone(), last_image_url.clone(), app_state.clone(), spotify_client.clone()).await.unwrap();
                    }
                }
            }
        });
        Ok(handle)
    }
}

async fn do_update(
    ccc: String,
    last_image_url: Arc<RwLock<String>>,
    app_state: web::Data<AppState>,
    spotify_client: web::Data<SpotifyClient>,
) -> Result<(), Box<dyn Error>> {
    let config = ElliConfig::from_ccc(&ccc)?;
    let elli_size = config.size;
    let mut connection = ElliConnection::new(config).await?;

    // fetch currently playing status from spotify
    let playing_model = if let Some(current_track) = spotify_client
        .get_current_track(ccc.as_str(), app_state)
        .await
        .map_err(ErrorInternalServerError)?
    {
        PlayingModel::from(current_track)
    } else {
        info!("No track playing for device: {}", ccc);
        return Ok(());
    };

    let current_url = playing_model.image_url.as_str();
    {
        let read_guard = last_image_url.read().await;
        if current_url == read_guard.as_str() {
            return Ok(()); // No change needed
        }
    } // read_guard is dropped here before we acquire the write lock

    let mut write_guard = last_image_url.write().await;
    *write_guard = current_url.to_string(); // or playing_model.image_url.clone()
    info!("Set last image url to: {}", write_guard.as_str());

    // only take the future and fetch the spotify data while the socket connection is established.
    let auth_future = connection.authenticate();

    // if something is playing, fetch the album art
    let image = spotify_client.get_image(&playing_model.image_url).await?;
    let downsized_image = image.resize(elli_size, elli_size, FilterType::Nearest);

    // await the authentication process of the lamp before we send pixels
    auth_future.await?;
    let mut throttle = interval(Duration::from_millis(20 * elli_size as u64));
    for (x, y, rgba) in downsized_image.pixels() {
        let data = PixelData::from_rgb(rgba[0], rgba[1], rgba[2], y as usize, x as usize);
        connection.write_pixel(data).await?;
        throttle.tick().await;
    }
    connection.close().await?;

    Ok(())
}
