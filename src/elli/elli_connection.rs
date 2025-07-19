use crate::elli::messages::websocket::{
    AuthMessage, AuthenticationMessage, PixelData, PixelMessage, RequestMessage, SocketMessage,
};
use crate::elli::{ConnectionStatus, ElliConfig};
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use serde_json::{from_str, to_string};
use std::error::Error;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

pub struct ElliConnection {
    cmd_tx: mpsc::Sender<Command>,
    close_manager_tx: oneshot::Sender<()>,
    close_receiver_tx: oneshot::Sender<()>,
    connection_status: ConnectionStatus,
    recv_join_handle: JoinHandle<()>,
    cmd_join_handle: JoinHandle<()>,
}

pub enum Command {
    Authenticate {
        resp: oneshot::Sender<Result<ConnectionStatus, CommandError>>,
    },
    WritePixel {
        data: PixelData,
        resp: oneshot::Sender<Result<(), CommandError>>,
    },
}

#[derive(Debug)]
pub struct CommandError {
    msg: String,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl Error for CommandError {}

impl ElliConnection {
    pub async fn new(config: ElliConfig) -> Result<Self, Box<dyn Error>> {
        info!("Connecting socket to: {}", config.host);
        let (ws_stream, _res) = connect_async(&config.host).await?;
        let (write, read) = ws_stream.split();
        let (tx_cmd, rx_cmd) = mpsc::channel(32);
        let (tx_recv, rx_recv) = mpsc::channel(32);
        let (tx_close_manager, rx_close_manager) = oneshot::channel();
        let (tx_close_recv, rx_close_recv) = oneshot::channel();
        let cmd_join_handle =
            ConnectionManager::new(write, config, rx_recv, rx_cmd, rx_close_manager).await;
        let recv_join_handle = ConnectionReceiver::new(read, tx_recv, rx_close_recv).await;

        let result = Self {
            cmd_tx: tx_cmd,
            cmd_join_handle,
            recv_join_handle,
            close_manager_tx: tx_close_manager,
            close_receiver_tx: tx_close_recv,
            connection_status: ConnectionStatus::Connected,
        };
        Ok(result)
    }

    pub async fn authenticate(&mut self) -> Result<(), Box<dyn Error>> {
        let (res_tx, res_rx) = oneshot::channel();
        let cmd = Command::Authenticate { resp: res_tx };
        self.cmd_tx.send(cmd).await?;
        let result = res_rx.await??;
        self.connection_status = result;
        info!("Authenticated Socket. Status: {:?}", self.connection_status);
        Ok(())
    }

    pub async fn write_pixel(&mut self, pixel: PixelData) -> Result<(), Box<dyn Error>> {
        let (res_tx, res_rx) = oneshot::channel();
        let cmd = Command::WritePixel {
            resp: res_tx,
            data: pixel,
        };
        self.cmd_tx.send(cmd).await?;
        let _ = res_rx.await??;
        Ok(())
    }

    pub async fn close(self) -> Result<(), Box<dyn Error>> {
        // send close signals
        let _ = self.close_receiver_tx.send(());
        let _ = self.close_manager_tx.send(());

        // wait for tasks to finish
        self.recv_join_handle.await?;
        self.cmd_join_handle.await?;

        info!("Socket finished closing");
        Ok(())
    }
}

type SocketReader = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;
type SocketWriter = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

enum RecvSocketMsg {
    Authentication { status: String },
}

struct ConnectionManager {
    writer: SocketWriter,
    config: ElliConfig,
    // possibly, we need a list inside the map in case we have multiple auth requests for the
    // same device
    pending_auth_request: Option<oneshot::Sender<Result<ConnectionStatus, CommandError>>>,
    // receiver to the socket reader
    rx_socket: mpsc::Receiver<RecvSocketMsg>,
    // receiver to receive commands from the main task
    rx_cmd: Receiver<Command>,
    // use oneshot channel for closing the manager
    rx_close: oneshot::Receiver<()>,
}

impl ConnectionManager {
    async fn new(
        writer: SocketWriter,
        config: ElliConfig,
        rx_socket: Receiver<RecvSocketMsg>,
        rx_cmd: Receiver<Command>,
        rx_close: oneshot::Receiver<()>,
    ) -> JoinHandle<()> {
        let result = Self {
            writer,
            config,
            pending_auth_request: None,
            rx_socket,
            rx_cmd,
            rx_close,
        };
        result.start_task().await
    }
    async fn start_task(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(cmd) = self.rx_cmd.recv() => { self.handle_recv_cmd(cmd).await }
                    Some(recv) = self.rx_socket.recv() => { self.handle_recv_socket_msg(recv).await }
                    _ = &mut self.rx_close => {
                        _ = self.writer.close().await; // we ignore the result and kill the task
                        break;
                    }
                }
            }
        })
    }

    async fn handle_recv_cmd(&mut self, cmd: Command) {
        match cmd {
            Command::Authenticate { resp } => {
                self.authenticate(resp).await;
            }
            Command::WritePixel { data, resp } => {
                self.write_pixel(data, resp).await;
            }
        }
    }

    async fn handle_recv_socket_msg(&mut self, msg: RecvSocketMsg) {
        match msg {
            RecvSocketMsg::Authentication { status } => {
                let connection_status = if status == "ok" {
                    ConnectionStatus::Authenticated
                } else {
                    ConnectionStatus::Error
                };

                if let Some(tx) = self.pending_auth_request.take() {
                    tx.send(Ok(connection_status)).unwrap();
                } else {
                    warn!("Received auth message from socket, but no pending async request in connection manager");
                }
            }
        }
    }

    async fn authenticate(
        &mut self,
        resp: oneshot::Sender<Result<ConnectionStatus, CommandError>>,
    ) {
        let auth_msg = AuthMessage {
            request: "authenticate".to_string(),
            param: "ReqL1".to_string(),
            device_type: "TetrisController".to_string(),
            address: self.config.d_code.clone(),
            from: self.config.b_code.clone(),
        };
        let msg = Utf8Bytes::from(to_string(&auth_msg).expect("Writing to json should work"));
        match self.writer.send(Message::Text(msg)).await {
            Ok(_) => {
                self.pending_auth_request = Some(resp);
            }
            Err(e) => {
                let command_error = CommandError {
                    msg: format!("{:?}", e),
                };
                resp.send(Err(command_error)).unwrap();
            }
        }
    }

    async fn write_pixel(
        &mut self,
        data: PixelData,
        resp: oneshot::Sender<Result<(), CommandError>>,
    ) {
        let req_msg = RequestMessage {
            request: String::from("write"),
            param: String::from("pixel"),
            from: self.config.b_code.clone(),
            to: self.config.d_code.clone(),
        };
        let pixel_msg = PixelMessage {
            pixel: data,
            request: req_msg,
        };

        let msg = Utf8Bytes::from(to_string(&pixel_msg).expect("Writing to json should work"));
        match self.writer.send(Message::Text(msg)).await {
            Ok(_) => {
                resp.send(Ok(())).unwrap();
            }
            Err(e) => {
                let command_error = CommandError {
                    msg: format!("{:?}", e),
                };
                resp.send(Err(command_error)).unwrap();
            }
        }
    }
}

pub struct ConnectionReceiver {
    reader: SocketReader,
    tx_recv: Sender<RecvSocketMsg>,
}

impl ConnectionReceiver {
    async fn new(
        reader: SocketReader,
        tx_recv: Sender<RecvSocketMsg>,
        rx_close: oneshot::Receiver<()>,
    ) -> JoinHandle<()> {
        let result = Self { reader, tx_recv };
        result.start_task(rx_close).await
    }

    async fn start_task(mut self, mut rx_close: oneshot::Receiver<()>) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    res = self.read_next() => {
                        if let Err(e) = res {
                            warn!("Error reading from socket: {:?}", e);
                        }
                    }
                    _ = &mut rx_close => {
                        break;
                    }
                }
            }
        })
    }

    pub async fn read_next(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(res) = self.reader.next().await {
            match res? {
                Message::Text(text) => self.handle_text(text.to_string()).await,
                Message::Ping(_) => {
                    info!("Received Ping");
                    Ok(())
                }
                Message::Close(c) => {
                    info!("Socket closed from other side: {:?}", c);
                    Ok(())
                }
                _ => Ok(()),
            }
        } else {
            Ok(())
        }
    }

    async fn handle_text(&mut self, text: String) -> Result<(), Box<dyn Error>> {
        let msg = from_str::<SocketMessage>(&text)?;
        match msg {
            SocketMessage::Authentication(a) => self.handle_authenticated(a).await?,
            SocketMessage::Write(_) => {
                warn!("Receiving write messages from socket server not implemented. Ignoring message.")
            }
        }
        Ok(())
    }

    async fn handle_authenticated(
        &mut self,
        msg: AuthenticationMessage,
    ) -> Result<(), SendError<RecvSocketMsg>> {
        let recv_msg = RecvSocketMsg::Authentication {
            status: msg.connection,
        };
        self.tx_recv.send(recv_msg).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_setup() {
        // Initialize logger to see info! messages
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .is_test(true)
            .init();

        let config = ElliConfig::from_ccc("0FBL3E2B3UPU4R9Z").expect("Failed to parse ccc");
        let mut connection = ElliConnection::new(config)
            .await
            .expect("Failed to create new socket connection");

        connection
            .authenticate()
            .await
            .map_err(|e| panic!("failed to authenticate with error: {}", e))
            .unwrap();

        let width = 5;
        let height = 5;
        let data = in_colors_data();
        for col in 0..height {
            for row in 0..width {
                let index = row * width + col;
                let rgb = data[index];
                let pixel = PixelData::from_rgb(rgb.0, rgb.1, rgb.2, row, col);
                connection
                    .write_pixel(pixel)
                    .await
                    .expect("Failed to send pixel");
            }
        }

        connection
            .close()
            .await
            .expect("Error while closing the connection");
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
