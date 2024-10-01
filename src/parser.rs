use crate::{Error, Result};

pub const CARRIAGE_RETURN: u8 = b'\r';
pub const LINE_FEED: u8 = b'\n';

#[derive(Debug)]
struct BufferLine {
    line: String,
    next_start: Option<usize>,
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
    TunnelRequest(RequestLine),
    TunnelResponse(ResponseLine),
    HttpRequest(RequestLine),
    HttpResponse(ResponseLine),
}

impl StatusLine {
    pub fn into_bytes(&self) -> Vec<u8> {
        match self {
            StatusLine::TunnelRequest(line) => line.into_bytes(),
            StatusLine::TunnelResponse(line) => line.into_bytes(),
            StatusLine::HttpRequest(line) => line.into_bytes(),
            StatusLine::HttpResponse(line) => line.into_bytes(),
        }
    }
}

fn parse_status_line(line: &str) -> Result<StatusLine> {
    let mut parts = line.split_whitespace();

    let Some(first) = parts.next() else {
        return Err(Error::TunnelStatusLineInvalid);
    };

    match first {
        "AUTH" | "FORWARD" => {
            // This is a tunnel request
            let st = RequestLine::parse(line)?;
            Ok(StatusLine::TunnelRequest(st))
        }
        "WEBHOOK/1.0" => {
            // This is a tunnel response
            let st = ResponseLine::parse(line)?;
            Ok(StatusLine::TunnelResponse(st))
        }
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" => {
            // This is a HTTP request
            let st = RequestLine::parse(line)?;
            Ok(StatusLine::HttpRequest(st))
        }
        "HTTP/1.1" | "HTTP/1.0" => {
            // This is a HTTP response
            // We don't support HTTP/2 yet, probably never
            let st = ResponseLine::parse(line)?;
            Ok(StatusLine::HttpResponse(st))
        }
        _ => Err(Error::TunnelStatusLineInvalid),
    }
}

#[derive(Debug)]
pub struct TunnelMessage {
    pub status_line: StatusLine,
    pub headers: Vec<HeaderItem>,
    pub initial_body: Vec<u8>,
}

impl TunnelMessage {
    pub fn new(status_line: StatusLine) -> Self {
        Self {
            status_line,
            headers: Vec::new(),
            initial_body: Vec::new(),
        }
    }

    pub fn with_tunnel_auth(token: String) -> Self {
        let st = StatusLine::TunnelRequest(RequestLine::new(
            "AUTH".to_string(),
            "/auth".to_string(),
            "WEBHOOK/1.0".to_string(),
        ));

        let mut req = Self::new(st);
        req.headers.push(("Authorization".to_string(), token));
        req
    }

    pub fn from_buffer(buffer: &[u8]) -> Result<Self> {
        // Read the request line
        let Some(req_bufline) = read_buffer_line(buffer, 0) else {
            return Err(Error::TunnelMessageInvalid);
        };

        // Do not parse the first line yet, assume it is valid
        let status_line = parse_status_line(req_bufline.line.as_str())?;

        let mut headers: Vec<HeaderItem> = Vec::new();
        let mut body_start: Option<usize> = None;

        // Message headers next
        if let Some(start) = req_bufline.next_start {
            let mut next_start = start;

            // Parse the remaining of the buffer for headers and possibly body
            while let Some(bufline) = read_buffer_line(buffer, next_start) {
                if bufline.line.is_empty() {
                    // End of headers
                    // See if we have a body
                    if let Some(b_start) = bufline.next_start {
                        body_start = Some(b_start);
                    }
                    break;
                }

                let Some((k, v)) = bufline.line.split_once(":") else {
                    return Err(Error::RequestHeaderInvalid);
                };

                // Simplify headers by forcing it be lowercase
                headers.push((k.to_lowercase(), v.trim().to_string()));

                match bufline.next_start {
                    Some(next) => {
                        next_start = next;
                    }
                    None => {
                        // No more headers
                        break;
                    }
                };
            }
        }

        let mut initial_body: Vec<u8> = Vec::new();
        if let Some(b_start) = body_start {
            // Consume the rest of the content as body
            initial_body.extend_from_slice(&buffer[b_start..]);
        }

        Ok(Self {
            status_line,
            headers,
            initial_body,
        })
    }

    pub fn is_tunnel_auth(&self) -> bool {
        match &self.status_line {
            StatusLine::TunnelRequest(line) => {
                line.method.as_str() == "AUTH"
                    && line.path.as_str() == "/auth"
                    && line.version.as_str() == "WEBHOOK/1.0"
            }
            _ => false,
        }
    }

    pub fn is_tunnel_request(&self) -> bool {
        match &self.status_line {
            StatusLine::TunnelRequest(_) => true,
            _ => false,
        }
    }

    pub fn is_tunnel_response(&self) -> bool {
        match &self.status_line {
            StatusLine::TunnelResponse(_) => true,
            _ => false,
        }
    }

    pub fn into_bytes(&self) -> Vec<u8> {
        let mut buffer: Vec<u8> = Vec::new();

        // Request line
        buffer.extend_from_slice(&self.status_line.into_bytes());

        // Headers
        for (k, v) in self.headers.iter() {
            let header_line = format!("{}: {}\r\n", k, v);
            buffer.extend_from_slice(&header_line.as_bytes());
        }

        // If there is a body, insert a blank line
        if self.initial_body.len() > 0 {
            buffer.extend_from_slice(b"\r\n");
            buffer.extend_from_slice(&self.initial_body);
        }

        buffer
    }
}

/// Reads a header buffer line and return the string and the next start position
fn read_buffer_line(buffer: &[u8], start: usize) -> Option<BufferLine> {
    let res = buffer
        .windows(2)
        .skip(start)
        .position(|w| w == &[CARRIAGE_RETURN, LINE_FEED]);

    if let Some(pos) = res {
        // Found it, let's parse the line
        let line = String::from_utf8_lossy(&buffer[start..(start + pos)]).to_string();
        let next_start: Option<usize> = if (start + pos + 2) < buffer.len() {
            Some(start + pos + 2)
        } else {
            None
        };

        return Some(BufferLine { line, next_start });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_buffer_line() {
        let buffer = format!(
            "{}\r\n{}\r\n",
            "AUTH /auth WEBHOOK/1.0", "Authorization: token"
        );

        let buffer_bytes = buffer.as_bytes();
        let res = read_buffer_line(buffer_bytes, 0);
        assert!(res.is_some());

        let req = res.expect("BufferLine must be created");
        assert_eq!(req.line.as_str(), "AUTH /auth WEBHOOK/1.0");
        assert_eq!(req.next_start, Some(24));

        let next_start = req.next_start.expect("Next start must be set");
        let auth_res = read_buffer_line(buffer_bytes, next_start);
        assert!(auth_res.is_some());

        let auth = auth_res.expect("BufferLine must be created");
        assert_eq!(auth.line.as_str(), "Authorization: token");
        assert_eq!(auth.next_start, None);
    }

    #[test]
    fn test_read_tunnel_request() {
        let buffer = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n",
            "AUTH /auth WEBHOOK/1.0",
            "Authorization: token",
            "User-Agent: rust-testing",
            "X-Foo-Bar:baz"
        );

        let buffer_bytes = buffer.as_bytes();
        let res = TunnelMessage::from_buffer(buffer_bytes);
        assert!(res.is_ok());

        let req = res.expect("TunnelMessage must be created");

        assert!(req.is_tunnel_request());

        match &req.status_line {
            StatusLine::TunnelRequest(line) => {
                assert_eq!(line.method.as_str(), "AUTH");
                assert_eq!(line.path.as_str(), "/auth");
                assert_eq!(line.version.as_str(), "WEBHOOK/1.0");
            }
            _ => panic!("Invalid status line"),
        }

        assert_eq!(req.headers.len(), 3);

        let mut headers = req.headers.iter();
        let auth = headers
            .next()
            .expect("Authorization header must be present");
        assert_eq!(auth.0.as_str(), "authorization");
        assert_eq!(auth.1.as_str(), "token");

        let ua = headers.next().expect("User-Agent header must be present");
        assert_eq!(ua.0.as_str(), "user-agent");
        assert_eq!(ua.1.as_str(), "rust-testing");

        let foo = headers.next().expect("X-Foo-Bar header must be present");
        assert_eq!(foo.0.as_str(), "x-foo-bar");
        assert_eq!(foo.1.as_str(), "baz");
    }

    #[test]
    fn test_read_tunnel_request_with_body() {
        let buffer = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "FORWARD /webhook WEBHOOK/1.0",
            "Authorization: token",
            "User-Agent: rust:testing",
            "X-Foo-Bar:baz",
            "",
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
            StatusLine::TunnelRequest(line) => {
                assert_eq!(line.method.as_str(), "FORWARD");
                assert_eq!(line.path.as_str(), "/webhook");
                assert_eq!(line.version.as_str(), "WEBHOOK/1.0");
            }
            _ => panic!("Invalid status line"),
        }

        assert_eq!(req.headers.len(), 3);

        let mut headers = req.headers.iter();
        let auth = headers
            .next()
            .expect("Authorization header must be present");
        assert_eq!(auth.0.as_str(), "authorization");
        assert_eq!(auth.1.as_str(), "token");

        let ua = headers.next().expect("User-Agent header must be present");
        assert_eq!(ua.0.as_str(), "user-agent");
        assert_eq!(ua.1.as_str(), "rust:testing");

        let foo = headers.next().expect("X-Foo-Bar header must be present");
        assert_eq!(foo.0.as_str(), "x-foo-bar");
        assert_eq!(foo.1.as_str(), "baz");

        assert_eq!(req.initial_body.len(), 91);
    }

    #[test]
    fn test_read_http_request_with_body() {
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
            StatusLine::HttpRequest(line) => {
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
    fn test_read_tunnel_response_with_body() {
        let buffer = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "WEBHOOK/1.0 200 OK",
            "",
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
            StatusLine::TunnelResponse(line) => {
                assert_eq!(line.version.as_str(), "WEBHOOK/1.0");
                assert_eq!(line.status_code, 200);
                assert_eq!(line.message.as_deref(), Some("OK"));
            }
            _ => panic!("Invalid status line"),
        }

        // No headers
        assert_eq!(res.headers.len(), 0);

        // Has body
        assert_eq!(res.initial_body.len(), 84);
    }

    #[test]
    fn test_read_http_response_with_body() {
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
            StatusLine::HttpResponse(line) => {
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
}
