mod connection;
pub mod messages;

use crate::elli::connection::{ElliReceiver, ElliSocket};
use crate::elli::messages::internal::Command;
use crate::elli::messages::websocket::PixelData;
use actix_web::error::ContentTypeError;
use actix_web::error::ContentTypeError::ParseError;
use futures_util::{SinkExt, StreamExt};
use log::info;
use std::error::Error;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;

pub struct ElliConfig {
    host: String,
    pub(crate) b_code: String,
    pub(crate) d_code: String,
    size: usize,
}

impl ElliConfig {
    pub fn new(host: String, b_code: String, d_code: String, size: usize) -> Self {
        info!(
            "new socket config with:{}, {}, {}, {}",
            host, b_code, d_code, size
        );
        Self {
            host,
            b_code,
            d_code,
            size,
        }
    }

    pub fn from_ccc(ccc: &str) -> Result<Self, ContentTypeError> {
        let (b_code, d_code, opt_size) = Self::parse_ccc(ccc)?;
        let host = String::from("wss://ws.elemon.de:443");
        let size = opt_size.unwrap_or(5);
        Ok(Self::new(host, b_code, d_code, size))
    }

    fn parse_ccc(ccc: &str) -> Result<(String, String, Option<usize>), ContentTypeError> {
        let b_code = ccc.get(0..8).ok_or(ParseError)?.to_string();
        let d_code = ccc.get(8..16).ok_or(ParseError)?.to_string();
        let size = ccc.get(16..18).and_then(|s| s.parse().ok());
        Ok((b_code, d_code, size))
    }
}

pub struct ElliConnection {
    command_tx: Mutex<tokio::sync::mpsc::Sender<Command>>,
    close_read: oneshot::Sender<()>,
    close_state: oneshot::Sender<()>,
    socket_handle: JoinHandle<()>,
    read_handle: JoinHandle<()>,
    size: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Offline,
    Connected,
    Error,
    Live,
    Authenticated,
}

impl ElliConnection {
    pub async fn new(config: ElliConfig) -> Result<Self, Box<dyn Error>> {
        info!("Connecting socket to: {}", config.host);
        let (ws_stream, _res) = connect_async(&config.host).await?;
        let (write, read) = ws_stream.split();
        let (close_read_tx, close_read_rx) = oneshot::channel();
        let (close_state_tx, close_state_rx) = oneshot::channel();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(32);
        let (on_recv_sender, on_recv_receiver) = tokio::sync::mpsc::channel(32);

        info!("Creating ElliReceiver");
        let recv = ElliReceiver {
            read,
            on_recv: on_recv_sender,
        };
        let read_handle = Self::start_socket_receive(recv, close_read_rx);
        let size = config.size; // copy size before config is moved

        info!("Creating ElliSocket");
        let state = ElliSocket {
            write,
            command_recv: command_rx,
            read_recv: on_recv_receiver,
            config,
            status: ConnectionStatus::Offline,
            device_name: String::new(),
        };
        let state_handle = Self::start_state_recv(state, close_state_rx);

        info!("Creating ElliConnection");
        let connection = ElliConnection {
            command_tx: Mutex::new(command_tx),
            close_read: close_read_tx,
            close_state: close_state_tx,
            socket_handle: state_handle,
            read_handle,
            size,
        };
        Ok(connection)
    }

    pub async fn send_pixel(&self, pixel: PixelData) -> Result<(), Box<dyn Error>> {
        let cmd = Command::WritePixel(pixel);
        self.command_tx.lock().await.send(cmd).await?;
        Ok(())
    }

    fn start_socket_receive(
        mut receiver: ElliReceiver,
        mut shutdown_receiver: oneshot::Receiver<()>,
    ) -> JoinHandle<()> {
        info!("Spawning task for socket receive");
        tokio::spawn(async move {
            info!(
                "Spawning select to listen to socket messages or shutdown signal of ElliReceiver"
            );
            loop {
                tokio::select! {
                    _ = &mut shutdown_receiver => {
                        // we want to shut down either when receiving the shutdown message or when
                        // the sender is dropped, which would cause an error
                      info!("ElliReceiver received shutdown signal.");
                        break;
                    }
                    res = receiver.receive_msg() => {
                        if let Err(e) = res {
                            info!("Error on receiving messages: {}", e);
                            break;
                        }
                    }
                }
            }
        })
    }

    fn start_state_recv(
        mut state: ElliSocket,
        mut shutdown: oneshot::Receiver<()>,
    ) -> JoinHandle<()> {
        info!("Spawning task for state receive");
        tokio::spawn(async move {
            // send auth message and then go into wait for commands and receive messages
            if let Err(e) = state.send_auth_msg().await {
                info!("Error on sending authentication message: {}", e);
                return;
            }
            info!("Spawning select to listen to signals for ElliSocket");
            loop {
                tokio::select! {
                    Some(cmd) = state.command_recv.recv() => {
                        state.on_cmd(cmd).await.expect("Error on executing command.");
                    }
                    Some(on_recv) = state.read_recv.recv() => {
                        state.on_rcv(on_recv).await.expect("Error on executing on_recv.");
                    }
                    _ = &mut shutdown => {
                        info!("ElliSocket received shutdown signal. Current state: {:?}, {:?}", state.status, state.device_name);
                        state.write.close().await.expect("Error on closing socket.");
                        info!("Socket closed.");
                        break;
                    }
                    else => {
                        info!("Else branch of start state recv");
                        break;
                    }
                }
            }
        })
    }

    pub async fn close(self) -> Result<(), Box<dyn Error>> {
        let _a = self.close_read.send(());
        let _b = self.close_state.send(());
        self.socket_handle.await?;
        self.read_handle.await?;
        Ok(())
    }

    pub fn size(&self) -> usize {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_elli_connection_authentication() {
        // Initialize logger to see info! messages
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .is_test(true)
            .init();

        let config = ElliConfig::from_ccc("0FBL3E2B3UPU4R9Z").expect("Failed to parse CCC");
        let connection = ElliConnection::new(config)
            .await
            .expect("Failed to create connection");

        sleep(Duration::from_secs(1)).await;

        connection
            .close()
            .await
            .expect("Failed to close connection");
    }

    #[tokio::test]
    async fn test_send_pixel() {
        // Initialize logger to see info! messages
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .is_test(true)
            .init();

        let config = ElliConfig::from_ccc("0FBL3E2B3UPU4R9Z").expect("Failed to parse CCC");
        let mut connection = ElliConnection::new(config)
            .await
            .expect("Failed to create connection");
        sleep(Duration::from_secs(1)).await;

        let width = 5;
        let height = 5;
        let data = in_colors_data();
        for col in 0..height {
            for row in 0..width {
                let index = row * width + col;
                let rgb = data[index];
                let pixel = PixelData::from_rgb(rgb.0, rgb.1, rgb.2, row, col);
                connection
                    .send_pixel(pixel)
                    .await
                    .expect("Failed to send pixel");
            }
        }

        sleep(Duration::from_secs(2)).await;
        connection
            .close()
            .await
            .expect("Failed to close connection");
    }

    fn in_colors_data() -> Vec<(u8, u8, u8)> {
        vec![
            (241, 142, 23),  // #f18e17
            (230, 71, 29),   // #e6471d
            (223, 10, 56),   // #df0a38
            (223, 6, 87),    // #df0657
            (228, 22, 122),  // #e4167a
            (251, 214, 20),  // #fbd614
            (241, 142, 23),  // #f18e17
            (223, 10, 56),   // #df0a38
            (228, 22, 122),  // #e4167a
            (216, 6, 129),   // #d80681
            (174, 202, 32),  // #aeca20
            (174, 202, 32),  // #aeca20
            (63, 69, 145),   // #3f4591
            (136, 31, 126),  // #881f7e
            (136, 31, 126),  // #881f7e
            (96, 178, 54),   // #60b236
            (255, 255, 255), // #ffffff
            (39, 132, 199),  // #2784c7
            (49, 54, 135),   // #313687
            (91, 37, 121),   // #5b2579
            (32, 155, 108),  // #209b6c
            (32, 161, 157),  // #20a19d
            (39, 132, 199),  // #2784c7
            (29, 97, 172),   // #1d61ac
            (49, 54, 135),   // #313687
        ]
    }
}
