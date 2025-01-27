use uuid::Uuid;

use crate::{Error, Result};

const CR: u8 = b'\r';
const LF: u8 = b'\n';

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
pub struct TunnelMessage {
    pub header: WebhookHeader,
    pub http_line: StatusLine,
    pub http_headers: Vec<HeaderItem>,
    pub http_body: Vec<u8>,
}

impl TunnelMessage {
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
}
