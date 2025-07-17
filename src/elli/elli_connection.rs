use crate::elli::messages::internal::OnRecv;
use crate::elli::messages::websocket::{AuthMessage, AuthenticationMessage, SocketMessage};
use crate::elli::{ConnectionStatus, ElliConfig};
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use serde_json::{from_str, to_string};
use std::collections::HashMap;
use std::error::Error;
use std::iter::Map;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot::error::RecvError;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

pub struct ElliConnection2 {
    cmd_tx: mpsc::Sender<Command>,
    ccc: String,
    auth_status: String,
}

pub enum Command {
    Authenticate {
        resp: oneshot::Sender<Result<String, CommandError>>,
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

impl ElliConnection2 {
    pub async fn new(config: ElliConfig) -> Result<Self, Box<dyn Error>> {
        info!("Connecting socket to: {}", config.host);
        let (ws_stream, _res) = connect_async(&config.host).await?;
        let (write, read) = ws_stream.split();
        let (tx_cmd, rx_cmd) = mpsc::channel(32);
        let (tx_recv, rx_recv) = mpsc::channel(32);
        let cm = ConnectionManager::new(write, config, rx_recv, rx_cmd).await;
        let cr = ConnectionReceiver::new(read, tx_recv).await;

        let result = Self {
            cmd_tx: tx_cmd,
            ccc: "".to_string(),
            auth_status: "".to_string(),
        };
        Ok(result)
    }

    pub async fn authenticate(&mut self) -> Result<(), Box<dyn Error>> {
        let (res_tx, res_rx) = oneshot::channel();
        let cmd = Command::Authenticate { resp: res_tx };
        self.cmd_tx.send(cmd).await?;
        let result = res_rx.await??;
        self.auth_status = result;
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
    pending_auth_request: Option<oneshot::Sender<Result<String, CommandError>>>,
    // receiver to the socket reader
    rx_socket: mpsc::Receiver<RecvSocketMsg>,
    // receiver to receive commands from the main task
    rx_cmd: Receiver<Command>,
}

impl ConnectionManager {
    async fn new(
        writer: SocketWriter,
        config: ElliConfig,
        rx_socket: Receiver<RecvSocketMsg>,
        rx_cmd: Receiver<Command>,
    ) -> JoinHandle<()> {
        let result = Self {
            writer,
            config,
            pending_auth_request: None,
            rx_socket,
            rx_cmd,
        };
        result.start_task().await
    }
    async fn start_task(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(cmd) = self.rx_cmd.recv() => { self.handle_recv_cmd(cmd).await }
                    Some(recv) = self.rx_socket.recv() => { self.handle_recv_socket_msg(recv).await }
                }
            }
        })
    }

    async fn handle_recv_cmd(&mut self, cmd: Command) {
        match cmd {
            Command::Authenticate { resp } => {
                self.authenticate(resp).await;
            }
        }
    }

    async fn handle_recv_socket_msg(&mut self, msg: RecvSocketMsg) {
        match msg {
            RecvSocketMsg::Authentication { status } => {
                if let Some(tx) = self.pending_auth_request.take() {
                    tx.send(Ok(status)).unwrap();
                } else {
                    warn!("Received auth message from socket, but no pending async request in connection manager");
                }
            }
        }
    }

    async fn authenticate(&mut self, resp: oneshot::Sender<Result<String, CommandError>>) {
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
}

pub struct ConnectionReceiver {
    reader: SocketReader,
    tx_recv: Sender<RecvSocketMsg>,
}

impl ConnectionReceiver {
    async fn new(reader: SocketReader, tx_recv: Sender<RecvSocketMsg>) -> JoinHandle<()> {
        let result = Self { reader, tx_recv };
        result.start_task().await
    }

    async fn start_task(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    res = self.read_next() => {
                        if let Err(e) = res {
                            warn!("Error reading from socket: {:?}", e);
                        }
                    }
                }
            }
        })
    }

    pub async fn read_next(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(res) = self.reader.next().await {
            match res? {
                Message::Text(text) => {}
                Message::Ping(p) => {
                    info!("Received Ping");
                    return Ok(());
                }
                Message::Close(c) => {
                    info!("Socket closed: {:?}", c);
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        Ok(())
    }

    async fn handle_text(&mut self, text: String) -> Result<(), Box<dyn Error>> {
        info!("Got text message: {}", text);
        let msg = from_str::<SocketMessage>(&text)?;
        match msg {
            SocketMessage::Authentication(a) => self.handle_authenticated(a).await?,
            SocketMessage::Write(_) => {
                panic!("write message not yet implemented")
            }
        }
        Ok(())
    }

    async fn handle_authenticated(
        &mut self,
        msg: AuthenticationMessage,
    ) -> Result<(), SendError<RecvSocketMsg>> {
        info!("Received authentication {}.", msg.connection);

        let recv_msg = RecvSocketMsg::Authentication {
            status: msg.connection,
        };

        self.tx_recv.send(recv_msg).await
    }
}
