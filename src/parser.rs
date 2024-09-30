pub type HeaderItem = (String, String);

pub struct RequestLine {
    pub method: String,
    pub path: String,
    pub version: String,
}

impl RequestLine {
    pub fn to_string(&self) -> String {
        format!("{} {} {}\r\n", self.method, self.path, self.version)
    }

    pub fn into_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

pub struct ResponseLine {
    pub version: String,
    pub status_code: u16,
    pub message: Option<String>,
}

impl ResponseLine {
    pub fn to_string(&self) -> String {
        format!(
            "{} {} {}\r\n",
            self.version,
            self.status_code,
            self.message.as_deref().unwrap_or("")
        )
    }

    pub fn into_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

pub struct TunnelRequestLine {
    pub method: String,
    pub path: String,
    pub version: String,
}

impl TunnelRequestLine {
    pub fn to_string(&self) -> String {
        format!("{} {} {}\r\n", self.method, self.path, self.version)
    }

    pub fn into_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

pub struct TunnelResponseLine {
    pub version: String,
    pub status_code: u16,
    pub message: Option<String>,
}

impl TunnelResponseLine {
    pub fn to_string(&self) -> String {
        format!(
            "{} {} {}\r\n",
            self.version,
            self.status_code,
            self.message.as_deref().unwrap_or("")
        )
    }

    pub fn into_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

pub struct TunnelRequest {
    pub request_line: TunnelRequestLine,
    pub headers: Vec<HeaderItem>,
    pub body_start: Option<usize>,
}

pub struct TunnelResponse {
    pub response_line: TunnelResponseLine,
    pub headers: Vec<HeaderItem>,
    pub body_start: Option<usize>,
}
