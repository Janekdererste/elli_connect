pub mod elli_connection;
pub mod messages;

use actix_web::error::ContentTypeError;
use actix_web::error::ContentTypeError::ParseError;
use log::info;

#[derive(Clone)]
pub struct ElliConfig {
    host: String,
    pub(crate) b_code: String,
    pub(crate) d_code: String,
    pub(crate) size: usize,
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

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Connected,
    Error,
    Authenticated,
}
