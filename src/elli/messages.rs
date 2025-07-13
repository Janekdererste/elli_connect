pub mod websocket {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    pub struct AuthMessage {
        pub(crate) request: String,
        pub(crate) param: String,
        #[serde(rename = "deviceType")]
        pub(crate) device_type: String,
        pub(crate) address: String,
        pub(crate) from: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct RequestMessage {
        pub(crate) request: String,
        pub(crate) param: String,
        pub(crate) from: String,
        pub(crate) to: String,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct PixelData {
        pub(crate) hue: u8,
        pub(crate) sat: u8,
        pub(crate) val: u8,
        pub(crate) row: usize,
        pub(crate) col: usize,
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
    pub enum SocketMessage {
        Authentication(AuthenticationMessage),
        Write(WriteMessage),
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct AuthenticationMessage {
        pub(crate) connection: String,
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
}

pub mod internal {
    use crate::elli::messages::websocket::PixelData;
    use crate::elli::ConnectionStatus;

    #[derive(Debug)]
    pub enum Command {
        WritePixel(PixelData),
    }

    #[derive(Debug)]
    pub enum OnRecv {
        Authentication {
            status: ConnectionStatus,
        },
        Disconnected {
            status: ConnectionStatus,
        },
        DeviceName {
            name: String,
            status: ConnectionStatus,
        },
        Pixel(PixelData),
    }

    pub struct MsgReceivedPayload {
        status: ConnectionStatus,
    }
}
