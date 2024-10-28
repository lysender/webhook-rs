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

// Tunnel message format
// Mesages are sent via websocket connection
// All binary messages from the server are assumed to be webhook requests
// All binary messages from the client are assumed to be webhook responses
// Each messages has a custom header line that looks like this:
//  WH/1.0 <message-id>\r\n
// The rest of the message contains the usual HTTP headers and body
#[derive(Debug)]
pub struct WebhookHeader {
    pub version: String,
    pub id: Uuid,
}

impl WebhookHeader {
    pub fn new(id: Uuid) -> Self {
        Self {
            version: "WH/1.0".to_string(),
            id,
        }
    }

    pub fn parse_str(line: &str) -> Result<Self> {
        let mut parts = line.split_whitespace();
        let Some(version) = parts.next() else {
            return Err(Error::TunnelMessageInvalid);
        };
        if version != "WH/1.0" {
            return Err(Error::TunnelMessageInvalid);
        }
        let Some(id) = parts.next() else {
            return Err(Error::TunnelMessageInvalid);
        };

        let Ok(id) = Uuid::parse_str(id) else {
            return Err(Error::TunnelMessageInvalid);
        };
        Ok(WebhookHeader {
            version: version.to_string(),
            id,
        })
    }

    pub fn to_string(&self) -> String {
        format!("{} {}\r\n", self.version, self.id.to_string())
    }

    pub fn into_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

#[derive(Debug)]
struct BufferHeader {
    data: String,
    body_start: Option<usize>,
}

type HeaderItem = (String, String);

#[derive(Debug)]
pub struct HttpLine {
    pub method: String,
    pub path: String,
    pub version: String,
}

impl HttpLine {
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
        Ok(HttpLine {
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
    Request(HttpLine),
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
            let st = HttpLine::parse(line)?;
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
pub struct TunnelMessage2 {
    pub header: WebhookHeader,
    pub http_line: StatusLine,
    pub http_headers: Vec<HeaderItem>,
    pub http_body: Vec<u8>,
}

impl TunnelMessage2 {
    pub fn new(header: WebhookHeader, http_line: StatusLine) -> Self {
        Self {
            header,
            http_line,
            http_headers: Vec::new(),
            http_body: Vec::new(),
        }
    }

    /// Parse the buffer for http headers and/or body
    pub fn from_buffer(buffer: &[u8]) -> Result<Self> {
        let buflen = buffer.len();

        let Some(buff_header) = read_buffer_header(&buffer[..buflen]) else {
            return Err(Error::TunnelMessageInvalid);
        };

        let mut lines = buff_header.data.lines();

        // First line is the webhook header
        let Some(header) = lines.next() else {
            return Err(Error::TunnelMessageInvalid);
        };
        let Ok(header) = WebhookHeader::parse_str(header.trim()) else {
            return Err(Error::TunnelMessageInvalid);
        };
        let Some(http_line) = lines.next() else {
            return Err(Error::TunnelMessageInvalid);
        };
        let http_line = parse_status_line(http_line.trim())?;

        let mut http_headers: Vec<HeaderItem> = Vec::new();

        // Read the rest of the headers
        for line in lines {
            let h_line = line.trim();
            if h_line.len() >= 3 {
                if let Some((k, v)) = h_line.split_once(":") {
                    http_headers.push((k.to_lowercase(), v.trim().to_string()));
                }
            }
        }

        let mut http_body: Vec<u8> = Vec::new();
        if let Some(b_start) = buff_header.body_start {
            // Consume the rest of the content as body
            http_body.extend_from_slice(&buffer[b_start..buflen]);
        }

        Ok(Self {
            header,
            http_line,
            http_headers,
            http_body,
        })
    }

    /// Converts full message into bytes, adding EOF marker at the end
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut buffer: Vec<u8> = Vec::new();

        // Webhook header
        buffer.extend_from_slice(&self.header.into_bytes());

        // Request line
        buffer.extend_from_slice(&self.http_line.into_bytes());

        // Headers
        for (k, v) in self.http_headers.iter() {
            let header_line = format!("{}: {}\r\n", k, v);
            buffer.extend_from_slice(&header_line.as_bytes());
        }

        // If there is a body, insert a blank line then the body
        if self.http_body.len() > 0 {
            buffer.extend_from_slice(b"\r\n");
            buffer.extend_from_slice(&self.http_body);
        }

        buffer
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
        let st = StatusLine::Request(HttpLine::new(
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

    pub fn from_large_buffer(buffer: &[u8]) -> Result<Vec<Self>> {
        let mut messages: Vec<Self> = Vec::new();
        let mut start = 0;
        let len = buffer.len();

        while start < len {
            let msg_pos = buffer[start..len]
                .windows(MSG_EOF.len())
                .position(|w| w == MSG_EOF);

            if let Some(pos) = msg_pos {
                // Marker found, this is probably a message
                let buf_start = start;
                let buf_end = start + pos + MSG_EOF.len();

                // Consume the range as a message
                let res = Self::from_buffer(&buffer[buf_start..buf_end]);
                match res {
                    Ok(msg) => {
                        messages.push(msg);

                        // Move on to the next message
                        start = buf_end;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                };
            } else {
                // It is possible that this buffer is too large that
                // it's body is partial, so we well consume the entire buffer instead
                let buf_start = start;
                let buf_end = len;
                let res = Self::from_buffer(&buffer[buf_start..buf_end]);
                match res {
                    Ok(msg) => {
                        messages.push(msg);

                        // We are done here
                        start = buf_end;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                };
            }
        }

        Ok(messages)
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

    /// Appends buffer to the body, returns next message pos if present
    pub fn accumulate_body(&mut self, buffer: &[u8]) -> Option<usize> {
        let eof_pos = buffer.windows(MSG_EOF.len()).position(|w| w == MSG_EOF);
        if let Some(pos) = eof_pos {
            // Found a marker, consume this range as additional body
            // This will not incldue the EOF marker it self
            self.initial_body.extend_from_slice(&buffer[..pos]);
            self.complete = true;

            let next_pos = pos + MSG_EOF.len();
            if next_pos < buffer.len() {
                return Some(next_pos);
            } else {
                // No more message left in this buffer
                return None;
            }
        } else {
            // No marker found, consume the entire buffer as body
            // Message is still not complete
            self.initial_body.extend_from_slice(buffer);
            return None;
        }
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
        let id = Uuid::now_v7();
        let id_header = format!("{}: {}", WEBHOOK_REQ_ID, id.to_string());
        let buffer = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "POST /webhook HTTP/1.1",
            "Content-Type: application/json",
            "Content-Length: 13",
            id_header.as_str(),
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

        assert_eq!(req.headers.len(), 3);

        let mut headers = req.headers.iter();
        let ctype = headers.next().expect("Content type header must be present");
        assert_eq!(ctype.0.as_str(), "content-type");
        assert_eq!(ctype.1.as_str(), "application/json");

        let clength = headers
            .next()
            .expect("Content length header must be present");
        assert_eq!(clength.0.as_str(), "content-length");
        assert_eq!(clength.1.as_str(), "13");

        let req_id = headers
            .next()
            .expect("Webhook Request ID header must be present");
        assert_eq!(req_id.0.as_str(), WEBHOOK_REQ_ID);
        assert_eq!(req_id.1.as_str(), id.to_string());

        assert_eq!(req.initial_body.len(), 13);
    }

    #[test]
    fn test_read_response_with_body() {
        let id = Uuid::now_v7();
        let id_header = format!("{}: {}", WEBHOOK_REQ_ID, id.to_string());

        let buffer = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "HTTP/1.1 200 OK",
            "Content-Type: application/json",
            "Content-Length: 13",
            id_header.as_str(),
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

        assert_eq!(res.headers.len(), 3);

        let mut headers = res.headers.iter();
        let ctype = headers.next().expect("Content type header must be present");
        assert_eq!(ctype.0.as_str(), "content-type");
        assert_eq!(ctype.1.as_str(), "application/json");

        let clength = headers
            .next()
            .expect("Content length header must be present");
        assert_eq!(clength.0.as_str(), "content-length");
        assert_eq!(clength.1.as_str(), "13");

        let req_id = headers
            .next()
            .expect("Webhook Request ID header must be present");
        assert_eq!(req_id.0.as_str(), WEBHOOK_REQ_ID);
        assert_eq!(req_id.1.as_str(), id.to_string());

        assert_eq!(res.initial_body.len(), 13);
    }

    #[test]
    fn test_message_with_eof() {
        let mut data = "FOO BAR BAZ\r\n".as_bytes().to_vec();
        data.extend_from_slice(MSG_EOF);

        let buffer = data.as_slice();
        assert!(buffer.ends_with(MSG_EOF));
    }

    #[test]
    fn test_multiple_complete_messages() {
        let eof = String::from_utf8_lossy(MSG_EOF).to_string();

        let id1 = Uuid::now_v7();
        let id1_header = format!("{}: {}", WEBHOOK_REQ_ID, id1.to_string());
        let body1 = format!("{}{}", "{\"foo\":\"bar\"}", eof);

        let buffer1 = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "HTTP/1.1 200 OK",
            "Content-Type: application/json",
            "Content-Length: 13",
            id1_header.as_str(),
            "",
            body1.as_str()
        );

        let id2 = Uuid::now_v7();
        let id2_header = format!("{}: {}", WEBHOOK_REQ_ID, id2.to_string());
        let body2 = format!("{}{}", "{\"foo\":\"bar\"}", eof);

        let buffer2 = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "HTTP/1.1 200 OK",
            "Content-Type: application/json",
            "Content-Length: 13",
            id2_header.as_str(),
            "",
            body2.as_str()
        );

        let full_buffer = format!("{}{}", buffer1, buffer2);
        let buffer_bytes = full_buffer.as_bytes();

        // Parse buffer to get 1 or more messages where the last message may be incomplete
        let messages =
            TunnelMessage::from_large_buffer(buffer_bytes).expect("Messages must be parsed");

        assert_eq!(messages.len(), 2);

        // First message
        let msg1 = &messages[0];
        assert_eq!(msg1.id, id1);
        assert!(msg1.complete);

        // Second message
        let msg2 = &messages[1];
        assert_eq!(msg2.id, id2);
        assert!(msg2.complete);
    }

    #[test]
    fn test_multiple_messages_last_incomplete() {
        let eof = String::from_utf8_lossy(MSG_EOF).to_string();

        let id1 = Uuid::now_v7();
        let id1_header = format!("{}: {}", WEBHOOK_REQ_ID, id1.to_string());
        let body1 = format!("{}{}", "{\"foo\":\"bar\"}", eof);

        let buffer1 = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "HTTP/1.1 200 OK",
            "Content-Type: application/json",
            "Content-Length: 13",
            id1_header.as_str(),
            "",
            body1.as_str()
        );

        let id2 = Uuid::now_v7();
        let id2_header = format!("{}: {}", WEBHOOK_REQ_ID, id2.to_string());
        let body2 = format!("{}", "{\"foo");

        let buffer2 = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "HTTP/1.1 200 OK",
            "Content-Type: application/json",
            "Content-Length: 13",
            id2_header.as_str(),
            "",
            body2.as_str()
        );

        let full_buffer = format!("{}{}", buffer1, buffer2);
        let buffer_bytes = full_buffer.as_bytes();

        // Parse buffer to get 1 or more messages where the last message may be incomplete
        let messages =
            TunnelMessage::from_large_buffer(buffer_bytes).expect("Messages must be parsed");

        assert_eq!(messages.len(), 2);

        // First message
        let msg1 = &messages[0];
        assert_eq!(msg1.id, id1);
        assert!(msg1.complete);

        // Second message
        let msg2 = &messages[1];
        assert_eq!(msg2.id, id2);
        assert!(!msg2.complete);
    }
}
