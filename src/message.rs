use uuid::Uuid;

use crate::{Error, Result};

const CR: u8 = b'\r';
const LF: u8 = b'\n';

// Added to the tunnel message body to indicate the end of the message
const MSG_EOF: &[u8] = b"<<[[((^_^))]]>>";

// Custom header names
pub const WEBHOOK_OP: &'static str = "x-webhook-op";
pub const WEBHOOK_TOKEN: &'static str = "x-webhook-token";
pub const WEBHOOK_REQ_ID: &'static str = "x-webhook-req-id";

// Custom header possible values
pub const WEBHOOK_OP_AUTH: &'static str = "auth";
pub const WEBHOOK_OP_AUTH_PATH: &'static str = "/_x_webhook_auth";
pub const WEBHOOK_OP_AUTH_RES: &'static str = "auth-res";
pub const WEBHOOK_OP_FORWARD: &'static str = "forward";
pub const WEBHOOK_OP_FORWARD_RES: &'static str = "forward-res";

#[derive(Debug)]
struct BufferHeader {
    data: String,
    body_start: Option<usize>,
}

type HeaderItem = (String, String);

#[derive(Debug)]
pub struct RequestLine {
    pub method: String,
    pub path: String,
    pub version: String,
}

impl RequestLine {
    pub fn new(method: String, path: String, version: String) -> Self {
        Self {
            method,
            path,
            version,
        }
    }

    pub fn parse(line: &str) -> Result<Self> {
        let mut parts = line.split_whitespace();
        let Some(method) = parts.next() else {
            return Err(Error::RequestLineInvalid("Method missing".to_string()));
        };
        let Some(path) = parts.next() else {
            return Err(Error::RequestLineInvalid("Path missing".to_string()));
        };
        let Some(version) = parts.next() else {
            return Err(Error::RequestLineInvalid("Version missing".to_string()));
        };
        Ok(RequestLine {
            method: method.to_string(),
            path: path.to_string(),
            version: version.to_string(),
        })
    }

    pub fn to_string(&self) -> String {
        format!("{} {} {}\r\n", self.method, self.path, self.version)
    }

    pub fn into_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

#[derive(Debug)]
pub struct ResponseLine {
    pub version: String,
    pub status_code: u16,
    pub message: Option<String>,
}

impl ResponseLine {
    pub fn new(version: String, status_code: u16, message: Option<String>) -> Self {
        Self {
            version,
            status_code,
            message,
        }
    }

    pub fn parse(line: &str) -> Result<Self> {
        let mut parts = line.split_whitespace();
        let Some(version) = parts.next() else {
            return Err(Error::ResponseLineInvalid("Version missing".to_string()));
        };
        let Some(status_code) = parts.next() else {
            return Err(Error::ResponseLineInvalid(
                "Status code missing".to_string(),
            ));
        };
        let message = parts.next().map(|s| s.to_string());

        Ok(ResponseLine {
            version: version.to_string(),
            status_code: status_code.parse().unwrap(),
            message,
        })
    }

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

#[derive(Debug)]
pub enum StatusLine {
    Request(RequestLine),
    Response(ResponseLine),
}

impl StatusLine {
    pub fn into_bytes(&self) -> Vec<u8> {
        match self {
            StatusLine::Request(line) => line.into_bytes(),
            StatusLine::Response(line) => line.into_bytes(),
        }
    }

    pub fn is_request(&self) -> bool {
        match self {
            StatusLine::Request(_) => true,
            _ => false,
        }
    }

    pub fn is_response(&self) -> bool {
        match self {
            StatusLine::Response(_) => true,
            _ => false,
        }
    }

    pub fn is_ok(&self) -> bool {
        match self {
            StatusLine::Response(line) => line.status_code == 200,
            _ => false,
        }
    }
}

fn parse_status_line(line: &str) -> Result<StatusLine> {
    let mut parts = line.split_whitespace();

    let Some(first) = parts.next() else {
        return Err(Error::TunnelStatusLineInvalid);
    };

    match first {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" => {
            let st = RequestLine::parse(line)?;
            Ok(StatusLine::Request(st))
        }
        "HTTP/1.1" | "HTTP/1.0" => {
            // We don't support HTTP/2 yet, probably never
            let st = ResponseLine::parse(line)?;
            Ok(StatusLine::Response(st))
        }
        _ => Err(Error::TunnelStatusLineInvalid),
    }
}

#[derive(Debug)]
pub struct TunnelMessage {
    pub id: Uuid,
    pub status_line: StatusLine,
    pub headers: Vec<HeaderItem>,
    pub initial_body: Vec<u8>,
    pub complete: bool,
}

impl TunnelMessage {
    pub fn new(id: Uuid, status_line: StatusLine) -> Self {
        // Assumes new message is complete
        Self {
            id,
            status_line,
            headers: vec![(WEBHOOK_REQ_ID.to_string(), id.to_string())],
            initial_body: Vec::new(),
            complete: true,
        }
    }

    /// Creates an auth request message
    pub fn with_auth_token(id: Uuid, token: String) -> Self {
        let st = StatusLine::Request(RequestLine::new(
            "POST".to_string(),
            WEBHOOK_OP_AUTH_PATH.to_string(),
            "HTTP/1.1".to_string(),
        ));

        let mut req = Self::new(id, st);
        req.headers
            .push((WEBHOOK_OP.to_string(), WEBHOOK_OP_AUTH.to_string()));
        req.headers.push((WEBHOOK_TOKEN.to_string(), token));
        req
    }

    pub fn with_auth_ok(id: Uuid) -> Self {
        let st = StatusLine::Response(ResponseLine::new(
            "HTTP/1.1".to_string(),
            200,
            Some("OK".to_string()),
        ));

        let mut res = Self::new(id, st);
        res.headers
            .push((WEBHOOK_OP.to_string(), WEBHOOK_OP_AUTH_RES.to_string()));
        res.initial_body = "OK".as_bytes().to_vec();
        res
    }

    pub fn with_auth_unauthorized(id: Uuid) -> Self {
        let st = StatusLine::Response(ResponseLine::new(
            "HTTP/1.1".to_string(),
            401,
            Some("Unauthorized".to_string()),
        ));

        let mut res = Self::new(id, st);
        res.headers
            .push((WEBHOOK_OP.to_string(), WEBHOOK_OP_AUTH_RES.to_string()));
        res.initial_body = "Unauthorized".as_bytes().to_vec();
        res
    }

    /// Parse the buffer for header data, include partial body if present
    pub fn from_buffer(buffer: &[u8]) -> Result<Self> {
        let mut complete = false;
        let mut buflen = buffer.len();

        if let Some(len_without_eof) = len_without_eof_marker(buffer, buflen) {
            buflen = len_without_eof;
            complete = true;
        }

        let Some(header) = read_buffer_header(&buffer[..buflen]) else {
            return Err(Error::TunnelMessageInvalid);
        };

        let mut lines = header.data.lines();
        let first_line = lines.next().unwrap().trim();
        let status_line = parse_status_line(first_line)?;

        let mut headers: Vec<HeaderItem> = Vec::new();

        let mut id: Option<Uuid> = None;

        // Read the rest of the headers
        for line in lines {
            let h_line = line.trim();
            if h_line.len() >= 3 {
                if let Some((k, v)) = h_line.split_once(":") {
                    let k = k.to_lowercase();
                    let v = v.trim().to_string();

                    if k.as_str() == WEBHOOK_REQ_ID {
                        id = Uuid::parse_str(v.as_str()).ok();
                    }
                    headers.push((k, v));
                }
            }
        }

        let id: Uuid = id.expect("Webhook Request ID must be present");

        let mut initial_body: Vec<u8> = Vec::new();
        if let Some(b_start) = header.body_start {
            // Consume the rest of the content as body
            initial_body.extend_from_slice(&buffer[b_start..buflen]);
        }

        Ok(Self {
            id,
            status_line,
            headers,
            initial_body,
            complete,
        })
    }

    pub fn is_auth(&self) -> bool {
        if !self.is_request() {
            return false;
        }

        let valid_status = match &self.status_line {
            StatusLine::Request(line) => {
                line.method.as_str() == "POST"
                    && line.path.as_str() == WEBHOOK_OP_AUTH_PATH
                    && line.version.as_str() == "HTTP/1.1"
            }
            _ => false,
        };

        if !valid_status {
            return false;
        }

        self.webhook_op()
            .map(|op| op == WEBHOOK_OP_AUTH)
            .unwrap_or(false)
    }

    pub fn is_auth_response(&self) -> bool {
        if !self.is_response() {
            return false;
        }

        self.webhook_op()
            .map(|op| op == WEBHOOK_OP_AUTH_RES)
            .unwrap_or(false)
    }

    pub fn is_forward(&self) -> bool {
        if !self.is_request() {
            return false;
        }

        self.webhook_op()
            .map(|op| op == WEBHOOK_OP_FORWARD)
            .unwrap_or(false)
    }

    pub fn is_request(&self) -> bool {
        self.status_line.is_request()
    }

    pub fn is_response(&self) -> bool {
        self.status_line.is_response()
    }

    pub fn webhook_op(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.as_str() == WEBHOOK_OP)
            .map(|(_, v)| v.as_str())
    }

    pub fn webhook_token(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.as_str() == WEBHOOK_TOKEN)
            .map(|(_, v)| v.as_str())
    }

    /// Appends data from a buffer to the body, returns true if body hits EOF
    pub fn accumulate_body(&mut self, buffer: &[u8]) -> bool {
        // Just accept the entire buffer as body to detect cut off EOF marker
        // Detect EOF marker from the entire body instead
        self.initial_body.extend_from_slice(&buffer);
        if let Some(len_eof) = len_without_eof_marker(&self.initial_body, self.initial_body.len()) {
            self.initial_body.truncate(len_eof);
            self.complete = true;
        }

        self.complete
    }

    /// Converts full message into bytes, adding EOF marker at the end
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut buffer: Vec<u8> = Vec::new();

        // Request line
        buffer.extend_from_slice(&self.status_line.into_bytes());

        // Headers
        for (k, v) in self.headers.iter() {
            let header_line = format!("{}: {}\r\n", k, v);
            buffer.extend_from_slice(&header_line.as_bytes());
        }

        // If there is a body, insert a blank line then the body
        if self.initial_body.len() > 0 {
            buffer.extend_from_slice(b"\r\n");
            buffer.extend_from_slice(&self.initial_body);
        }

        buffer.extend_from_slice(MSG_EOF);

        buffer
    }
}

// Parses buffer for header data, include body start index if present
fn read_buffer_header(buffer: &[u8]) -> Option<BufferHeader> {
    if buffer.len() == 0 {
        return None;
    }

    // Find the body separator
    if let Some(pos) = buffer.windows(4).position(|w| w == [CR, LF, CR, LF]) {
        // We got a body, parse the entire header as string,
        // and the starting position of the body
        let data = String::from_utf8_lossy(&buffer[..pos]).to_string();
        let body_start: Option<usize> = if (pos + 4) < buffer.len() {
            Some(pos + 4)
        } else {
            None
        };

        return Some(BufferHeader { data, body_start });
    }

    // The entire data might be all headers
    let data = String::from_utf8_lossy(&buffer).to_string();
    Some(BufferHeader {
        data,
        body_start: None,
    })
}

fn len_without_eof_marker(buffer: &[u8], n: usize) -> Option<usize> {
    let partial = &buffer[..n];
    if partial.ends_with(MSG_EOF) {
        let reduced_len = n - MSG_EOF.len();
        if reduced_len > 0 && reduced_len < n {
            return Some(reduced_len);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_buffer_header() {
        let data = format!("{}\r\n{}\r\n", "GET / HTTP/1.1", "User-Agent: Insomia");
        let buffer = data.as_bytes();

        let res = read_buffer_header(buffer);
        assert!(res.is_some());

        let header = res.unwrap();
        assert_eq!(header.data.len(), 37);
        assert!(header.body_start.is_none());
    }

    #[test]
    fn test_read_buffer_header_with_body() {
        let data = format!(
            "{}\r\n{}\r\n\r\n{}",
            "POST /auth HTTP/1.1", "User-Agent: Disturbia", "username=rihanna"
        );
        let buffer = data.as_bytes();

        let res = read_buffer_header(buffer);
        assert!(res.is_some());

        let header = res.unwrap();
        assert_eq!(header.data.len(), 42);
        assert!(header.body_start.is_some());

        let body_start = header.body_start.unwrap();
        assert_eq!(body_start, 46);
    }

    #[test]
    fn test_read_request_with_body() {
        let buffer = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "POST /webhook HTTP/1.1",
            "Content-Type: application/json",
            "Content-Length: 13",
            "",
            "{\"foo\":\"bar\"}",
        );

        let buffer_bytes = buffer.as_bytes();
        let res = TunnelMessage::from_buffer(buffer_bytes);
        assert!(res.is_ok());

        let req = res.expect("TunnelMessage must be created");

        match &req.status_line {
            StatusLine::Request(line) => {
                assert_eq!(line.method.as_str(), "POST");
                assert_eq!(line.path.as_str(), "/webhook");
                assert_eq!(line.version.as_str(), "HTTP/1.1");
            }
            _ => panic!("Invalid status line"),
        }

        assert_eq!(req.headers.len(), 2);

        let mut headers = req.headers.iter();
        let auth = headers.next().expect("Content type header must be present");
        assert_eq!(auth.0.as_str(), "content-type");
        assert_eq!(auth.1.as_str(), "application/json");

        let ua = headers
            .next()
            .expect("Content length header must be present");
        assert_eq!(ua.0.as_str(), "content-length");
        assert_eq!(ua.1.as_str(), "13");

        assert_eq!(req.initial_body.len(), 13);
    }

    #[test]
    fn test_read_response_with_body() {
        let buffer = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "HTTP/1.1 200 OK",
            "Content-Type: application/json",
            "Content-Length: 13",
            "",
            "{\"foo\":\"bar\"}",
        );

        let buffer_bytes = buffer.as_bytes();
        let res = TunnelMessage::from_buffer(buffer_bytes);
        assert!(res.is_ok());

        let res = res.expect("TunnelMessage must be created");

        match &res.status_line {
            StatusLine::Response(line) => {
                assert_eq!(line.version.as_str(), "HTTP/1.1");
                assert_eq!(line.status_code, 200);
                assert_eq!(line.message.as_deref(), Some("OK"));
            }
            _ => panic!("Invalid status line"),
        }

        assert_eq!(res.headers.len(), 2);

        let mut headers = res.headers.iter();
        let auth = headers.next().expect("Content type header must be present");
        assert_eq!(auth.0.as_str(), "content-type");
        assert_eq!(auth.1.as_str(), "application/json");

        let ua = headers
            .next()
            .expect("Content length header must be present");
        assert_eq!(ua.0.as_str(), "content-length");
        assert_eq!(ua.1.as_str(), "13");

        assert_eq!(res.initial_body.len(), 13);
    }

    #[test]
    fn test_message_with_eof() {
        let mut data = "FOO BAR BAZ\r\n".as_bytes().to_vec();
        data.extend_from_slice(MSG_EOF);

        let buffer = data.as_slice();
        assert!(buffer.ends_with(MSG_EOF));
    }
}
