use crate::elli::ConnectionStatus::Connected;
use actix_web::error::ContentTypeError::ParseError;
use futures_util::{SinkExt, StreamExt};
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string};
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

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

#[derive(Debug, Serialize, Deserialize)]
struct RequestMessage {
    request: String,
    param: String,
    from: String,
    to: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PixelData {
    hue: u8,
    sat: u8,
    val: u8,
    row: usize,
    col: usize,
}

impl PixelData {
    pub fn from_rgb(r: u8, g: u8, b: u8, row: usize, col: usize) -> Self {
        let (hue, sat, val) = Self::rgb_to_hsv(r, g, b);
        Self {
            hue,
            sat,
            val,
            row,
            col,
        }
    }

    fn diff_c(c: f32, v: f32, diff: f32) -> f32 {
        (v - c) / 6.0 / diff + 0.5
    }

    fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
        let rabs: f32 = r as f32 / 255.;
        let gabs: f32 = g as f32 / 255.;
        let babs: f32 = b as f32 / 255.;
        let v = rabs.max(gabs).max(babs);
        let mut h: f32 = 0.0;
        let mut s: f32 = 0.0;

        let diff = v - rabs.min(gabs).min(babs);
        if diff == 0. {
            h = 0.0;
            s = 0.0;
        } else {
            s = diff / v;
            let rr = Self::diff_c(rabs, v, diff);
            let gg = Self::diff_c(gabs, v, diff);
            let bb = Self::diff_c(babs, v, diff);

            if rabs == v {
                h = bb - gg;
            } else if gabs == v {
                h = 1.0 / 3.0 + rr - bb;
            } else if babs == v {
                h = 2.0 / 3.0 + gg - rr;
            }
            if h < 0.0 {
                h += 1.0;
            } else if h > 1.0 {
                h -= 1.0;
            }
        }
        let h_abs = h * 255.0;
        let s_abs = s * 255.0;
        let v_abs = v * 255.0;
        (
            h_abs.round() as u8,
            s_abs.round() as u8,
            v_abs.round() as u8,
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PixelMessage {
    #[serde(flatten)]
    pub pixel: PixelData,
    #[serde(flatten)]
    pub request: RequestMessage,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged, rename_all = "lowercase")]
enum SocketMessage {
    Authentication(AuthenticationMessage),
    Write(WriteMessage),
}

#[derive(Debug, Deserialize, Serialize)]
struct AuthenticationMessage {
    connection: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "param")]
pub enum WriteMessage {
    #[serde(rename = "name")]
    DeviceName(DeviceNameMessage),
    #[serde(rename = "pixel")]
    Pixel(PixelMessage),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeviceNameMessage {
    pub request: String,
    pub name: String,
    pub to: String,
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
        let (ws_stream, _res) = connect_async(&config.host).await?;
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

    async fn send_pixels(&mut self, pixels: Vec<PixelData>) -> Result<(), Box<dyn Error>> {
        // TODO implement pixel writing.
        // create some message
        for pixel in pixels.into_iter() {
            let request = PixelMessage {
                pixel,
                request: RequestMessage {
                    request: String::from("write"),
                    param: String::from("pixel"),
                    from: self.config.b_code.clone(),
                    to: self.config.d_code.clone(),
                },
            };

            let message = Message::Text(Utf8Bytes::from(to_string(&request)?));
            info!("Sending pixel message: {:#?}", message);
            self.write.send(message).await?;
        }
        Ok(())
    }

    async fn request_name(&mut self) -> Result<(), Box<dyn Error>> {
        let request = RequestMessage {
            request: String::from("read"),
            param: String::from("name"),
            from: self.config.b_code.clone(),
            to: self.config.d_code.clone(),
        };
        info!("Sending name request message");
        let message = Utf8Bytes::from(to_string(&request)?);
        self.write.send(Message::Text(message)).await?;
        info!("After sending name request message");
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
            SocketMessage::Authentication(msg) => self.handle_authenticated(msg).await?,
            SocketMessage::Write(msg) => self.handle_write(msg),
        }

        Ok(())
    }

    async fn handle_authenticated(
        &mut self,
        msg: AuthenticationMessage,
    ) -> Result<(), Box<dyn Error>> {
        info!("Received authentication message.");
        if &msg.connection == "ok" {
            self.status = ConnectionStatus::Authenticated;
            self.request_name().await?;
            Ok(())
        } else {
            self.status = ConnectionStatus::Error;
            Err("Authentication failed.".into())
        }
    }

    fn handle_write(&mut self, msg: WriteMessage) {
        match msg {
            WriteMessage::DeviceName(d_msg) => self.handle_device_name(d_msg),
            WriteMessage::Pixel(p_msg) => self.handle_pixel(p_msg),
        }
    }

    fn handle_device_name(&mut self, msg: DeviceNameMessage) {
        info!("Received device name message: {} to {}", msg.name, msg.to);
        self.status = ConnectionStatus::Live;
    }

    fn handle_pixel(&mut self, msg: PixelMessage) {
        info!(
            "Received pixel message: {} {} {} {} {} {}",
            msg.pixel.row,
            msg.pixel.col,
            msg.pixel.hue,
            msg.pixel.sat,
            msg.pixel.val,
            msg.request.to
        );
    }

    fn handle_close(&mut self) {
        self.status = ConnectionStatus::Offline;
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

        tokio::time::timeout(Duration::from_secs(5), async {
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

    #[tokio::test]
    async fn test_send_pixel() {
        // Initialize logger to see info! messages
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .is_test(true)
            .init();

        let config = SocketConfig::from_ccc("0FBL3E2B3UPU4R9Z").expect("Failed to parse CCC");
        let mut socket = ElliSocket::new(config)
            .await
            .expect("Failed to create socket");

        // connect to the server
        tokio::time::timeout(Duration::from_secs(5), async {
            while socket.status != ConnectionStatus::Live {
                socket
                    .receive_message()
                    .await
                    .expect("Failed to receive message");
            }
        })
        .await
        .expect("Authentication timed out");

        let width = 5;
        let height = 5;
        let data = in_colors_data();
        let mut pixels = Vec::new();
        for col in 0..height {
            for row in 0..width {
                let index = row * width + col;
                let rgb = data[index];
                let pixel = PixelData::from_rgb(rgb.0, rgb.1, rgb.2, row, col);
                pixels.push(pixel);
            }
        }

        socket
            .send_pixels(pixels)
            .await
            .expect("Failed to send pixels");
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
