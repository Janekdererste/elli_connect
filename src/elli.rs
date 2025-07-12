use crate::elli::ConnectionStatus::Connected;
use actix_web::error::ContentTypeError::ParseError;
use futures_util::{SinkExt, StreamExt};
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string};
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;

pub struct SocketConfig {
    host: String,
    b_code: String,
    d_code: String,
    size: usize,
}

impl SocketConfig {
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

    pub fn from_ccc(ccc: &str) -> Result<Self, Box<dyn Error>> {
        let (b_code, d_code, opt_size) = Self::parse_ccc(ccc)?;
        let host = String::from("wss://ws.elemon.de:443");
        let size = opt_size.unwrap_or(5);
        Ok(Self::new(host, b_code, d_code, size))
    }

    fn parse_ccc(ccc: &str) -> Result<(String, String, Option<usize>), Box<dyn Error>> {
        let b_code = ccc.get(0..8).ok_or(ParseError)?.to_string();
        let d_code = ccc.get(8..16).ok_or(ParseError)?.to_string();
        let size = ccc.get(16..18).and_then(|s| s.parse().ok());
        Ok((b_code, d_code, size))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Offline,
    Connected,
    Error,
    Live,
    Authenticated,
}

#[derive(Serialize, Deserialize)]
struct AuthMessage {
    request: String,
    param: String,
    #[serde(rename = "deviceType")]
    device_type: String,
    address: String,
    from: String,
}

#[derive(Serialize, Deserialize)]
struct RequestMessage {
    request: String,
    param: String,
    from: String,
    to: String,
}

impl RequestMessage {
    pub fn new(req_type: String, from: String, to: String) -> Self {
        Self {
            request: String::from("read"),
            param: req_type,
            from,
            to,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SocketMessage {
    Authentication {
        connection: String,
    },
    Write {
        request: String,
        param: String,
        #[serde(flatten)] // This flattens the remaining fields into the struct
        params: WriteParams,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum WriteParams {
    DeviceName {
        name: String,
        to: String,
    },
    Pixel {
        row: u32,
        col: u32,
        hue: u8,
        sat: u8,
        val: u8,
        to: String,
    },
}

#[derive(Deserialize)]
struct WebSocketMessage {
    connection: Option<String>,
    // Add other fields as needed
}

struct WriteSocketMessage {}

pub struct ElliSocket {
    write: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    read: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    config: SocketConfig,
    status: ConnectionStatus,
}

impl ElliSocket {
    pub async fn new(config: SocketConfig) -> Result<Self, Box<dyn Error>> {
        info!("Connecting socket to: {}", config.host);
        let (ws_stream, res) = connect_async(&config.host).await?;
        let (write, read) = ws_stream.split();

        info!("Socket connected.");
        let mut result = Self {
            write,
            read,
            config,
            status: Connected,
        };
        result.send_auth_msg().await?;
        Ok(result)
    }

    async fn send_auth_msg(&mut self) -> Result<(), Box<dyn Error>> {
        let auth_message = AuthMessage {
            request: "authenticate".to_string(),
            param: "ReqL1".to_string(),
            device_type: "TetrisController".to_string(),
            address: self.config.d_code.clone(),
            from: self.config.b_code.clone(),
        };

        info!("Sending authentication message");
        let message = Utf8Bytes::from(to_string(&auth_message)?);
        self.write.send(Message::Text(message)).await?;
        info!("After sending authentication message");
        Ok(())
    }

    async fn request_name(&mut self) -> Result<(), Box<dyn Error>> {
        let request_name = RequestMessage::new(
            String::from("name"),
            self.config.b_code.clone(),
            self.config.d_code.clone(),
        );
        info!("Sending name request message");
        let message = Utf8Bytes::from(to_string(&request_name)?);
        self.write.send(Message::Text(message)).await?;
        info!("After sending name request message");
        Ok(())
    }

    pub async fn send_message(&mut self, message: &str) -> Result<(), Box<dyn Error>> {
        self.write
            .send(Message::Text(Utf8Bytes::from(message)))
            .await?;
        Ok(())
    }

    pub async fn receive_message(&mut self) -> Result<(), Box<dyn Error>> {
        info!("Received message is called.");
        if let Some(read_result) = self.read.next().await {
            info!("Got read result.");
            match read_result? {
                Message::Text(text) => self.handle_text_msg(text.to_string()).await,
                Message::Close(_) => {
                    info!("Received close message.");
                    self.handle_close();
                    Ok(())
                }
                Message::Binary(b) => {
                    info!("Received binary message: {b:#?}");
                    Ok(())
                }
                Message::Ping(p) => {
                    info!("Received ping: {p:#?}");
                    Ok(())
                }
                Message::Pong(p) => {
                    info!("Received pong: {p:#?}");
                    Ok(())
                }
                Message::Frame(f) => {
                    info!("Received frame: {f:#?}");
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

    async fn handle_text_msg(&mut self, text: String) -> Result<(), Box<dyn Error>> {
        info!("Got text message: {}", text);
        let msg = from_str::<SocketMessage>(&text)?;
        match msg {
            SocketMessage::Authentication { .. } => {
                info!("Received authentication message.");
                self.status = ConnectionStatus::Authenticated;
                self.request_name().await?;
            }
            SocketMessage::Write {
                request,
                param,
                params,
            } => match params {
                WriteParams::DeviceName { name, to } => {
                    info!("Received device name message: {} to {}", name, to);
                    self.status = ConnectionStatus::Live;
                }
                WriteParams::Pixel {
                    row,
                    col,
                    hue,
                    sat,
                    val,
                    to,
                } => {
                    info!(
                        "Received pixel message: {} {} {} {} {} {}",
                        row, col, hue, sat, val, to
                    );
                }
            },
        }

        Ok(())
    }

    fn handle_close(&mut self) {
        self.status = ConnectionStatus::Offline;
    }

    fn is_connected(msg: &WebSocketMessage) -> bool {
        if let Some(status) = &msg.connection {
            status == "ok"
        } else {
            false
        }
    }
}

pub struct ElliConnections {
    connections: Arc<RwLock<HashMap<String, ElliSocket>>>,
}

impl ElliConnections {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // pub async fn add_connection(&self, user_id: &str) -> Result<(), Box<dyn Error>> {
    //     let socket = ElliSocket::new(user_id).await?;
    //     let mut connections = self.connections.write().await;
    //     connections.insert(String::from(user_id), socket);
    //     Ok(())
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_elli_connection_authentication() {
        // Initialize logger to see info! messages
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .is_test(true)
            .init();

        let config = SocketConfig::from_ccc("0FBL3E2B3UPU4R9Z").expect("Failed to parse CCC");
        let mut socket = ElliSocket::new(config)
            .await
            .expect("Failed to create socket");
        // socket
        //     .send_auth_msg()
        //     .await
        //     .expect("Failed to send authentication message");

        tokio::time::timeout(Duration::from_secs(50), async {
            while socket.status != ConnectionStatus::Live {
                socket
                    .receive_message()
                    .await
                    .expect("Failed to receive message");
            }
        })
        .await
        .expect("Timeout timed out.");
    }
}
