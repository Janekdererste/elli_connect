use crate::elli::messages::internal::{Command, OnRecv};
use crate::elli::messages::websocket::{
    AuthMessage, AuthenticationMessage, DeviceNameMessage, PixelData, PixelMessage, RequestMessage,
    SocketMessage, WriteMessage,
};
use crate::elli::{ConnectionStatus, SocketConfig};
use futures_util::{SinkExt, StreamExt};
use log::info;
use serde_json::{from_str, to_string};
use std::error::Error;
use tokio::sync::mpsc::error::SendError;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

pub struct ElliReceiver {
    pub(crate) read: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    pub(crate) on_recv: tokio::sync::mpsc::Sender<OnRecv>,
}

impl ElliReceiver {
    pub async fn receive_msg(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(read_result) = self.read.next().await {
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
            SocketMessage::Write(msg) => self.handle_write(msg).await?,
        }
        Ok(())
    }

    async fn handle_authenticated(
        &mut self,
        msg: AuthenticationMessage,
    ) -> Result<(), SendError<OnRecv>> {
        info!("Received authentication {}.", msg.connection);
        let status = if &msg.connection == "ok" {
            ConnectionStatus::Authenticated
        } else {
            ConnectionStatus::Error
        };

        self.on_recv.send(OnRecv::Authentication { status }).await
    }

    async fn handle_write(&mut self, msg: WriteMessage) -> Result<(), SendError<OnRecv>> {
        match msg {
            WriteMessage::DeviceName(d_msg) => self.handle_device_name(d_msg).await,
            WriteMessage::Pixel(p_msg) => {
                self.handle_pixel(p_msg);
                Ok(())
            }
        }
    }

    async fn handle_device_name(
        &mut self,
        msg: DeviceNameMessage,
    ) -> Result<(), SendError<OnRecv>> {
        info!("Received device name message: {} to {}", msg.name, msg.to);

        let on_recv = OnRecv::DeviceName {
            name: String::from(msg.name),
            status: ConnectionStatus::Live,
        };
        self.on_recv.send(on_recv).await
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

    async fn handle_close(&mut self) -> Result<(), SendError<OnRecv>> {
        self.on_recv
            .send(OnRecv::Disconnected {
                status: ConnectionStatus::Offline,
            })
            .await
    }
}

pub struct ElliSocket {
    pub(crate) write: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    pub(crate) command_recv: tokio::sync::mpsc::Receiver<Command>,
    pub(crate) read_recv: tokio::sync::mpsc::Receiver<OnRecv>,
    pub(crate) config: SocketConfig,
    pub(crate) status: ConnectionStatus,
    pub(crate) device_name: String,
}

impl ElliSocket {
    pub(crate) async fn on_cmd(&mut self, cmd: Command) -> Result<(), Box<dyn Error>> {
        match cmd {
            Command::WritePixel(p) => self.send_pixel(p).await,
        }
    }

    pub(crate) async fn on_rcv(&mut self, on_recv: OnRecv) -> Result<(), Box<dyn Error>> {
        match on_recv {
            OnRecv::Authentication { status } => self.handle_authentication(status).await,
            OnRecv::Disconnected { status } => self.handle_disconnected(status),
            OnRecv::DeviceName { status, name } => {
                self.handle_device_name(status, name);
                Ok(())
            }
            OnRecv::Pixel(p) => {
                self.handle_pixel(p);
                Ok(())
            }
        }
    }

    async fn handle_authentication(
        &mut self,
        status: ConnectionStatus,
    ) -> Result<(), Box<dyn Error>> {
        self.status = status;
        info!("Handle authentication status: {:?}", self.status);
        if self.status == ConnectionStatus::Error {
            Err("Authentication failed.".into())
        } else {
            self.request_name().await?;
            Ok(())
        }
    }

    fn handle_disconnected(&mut self, status: ConnectionStatus) -> Result<(), Box<dyn Error>> {
        info!("Handle disconnected status: {:?}", self.status);
        self.status = status;
        if self.status == ConnectionStatus::Error {
            Err("Elli disconnected because of an error".into())
        } else {
            Ok(())
        }
    }

    fn handle_device_name(&mut self, status: ConnectionStatus, name: String) {
        info!(
            "Received device name message: {:?} status {:?}",
            name, status
        );
        self.status = status;
        self.device_name = name;
    }

    fn handle_pixel(&mut self, pixel: PixelData) {
        info!(
            "Received pixel message: {} {} {} {} {}",
            pixel.row, pixel.col, pixel.hue, pixel.sat, pixel.val
        );
    }

    pub(crate) async fn send_auth_msg(&mut self) -> Result<(), Box<dyn Error>> {
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

    async fn send_pixel(&mut self, pixel: PixelData) -> Result<(), Box<dyn Error>> {
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

    fn handle_close(&mut self) {
        self.status = ConnectionStatus::Offline;
    }
}
