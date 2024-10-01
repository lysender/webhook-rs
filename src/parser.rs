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
pub struct TunnelRequest {
    pub request_line: RequestLine,
    pub headers: Vec<HeaderItem>,
    pub initial_body: Vec<u8>,
}

impl TunnelRequest {
    pub fn from_buffer(buffer: &[u8]) -> Result<Self> {
        // Read the request line
        let Some(req_bufline) = read_buffer_line(buffer, 0) else {
            return Err(Error::RequestLineInvalid(
                "Request line missing".to_string(),
            ));
        };

        let request_line = RequestLine::parse(&req_bufline.line)?;
        let mut headers: Vec<HeaderItem> = Vec::new();
        let mut body_start: Option<usize> = None;

        // Read headers next
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

                let parts: Vec<&str> = bufline.line.split(":").collect();
                if parts.len() != 2 {
                    return Err(Error::RequestHeaderInvalid);
                }

                headers.push((parts[0].to_string(), parts[1].trim().to_string()));

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
            request_line,
            headers,
            initial_body,
        })
    }
}

#[derive(Debug)]
pub struct TunnelResponse {
    pub response_line: ResponseLine,
    pub headers: Vec<HeaderItem>,
    pub initial_body: Vec<u8>,
}

impl TunnelResponse {
    pub fn from_buffer(buffer: &[u8]) -> Result<Self> {
        // Read the request line
        let Some(req_bufline) = read_buffer_line(buffer, 0) else {
            return Err(Error::ResponseLineInvalid(
                "Response line missing".to_string(),
            ));
        };

        let response_line = ResponseLine::parse(&req_bufline.line)?;
        let mut headers: Vec<HeaderItem> = Vec::new();
        let mut body_start: Option<usize> = None;

        // Read headers next
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

                let parts: Vec<&str> = bufline.line.split(":").collect();
                if parts.len() != 2 {
                    return Err(Error::ResponseHeaderInvalid);
                }

                headers.push((parts[0].to_string(), parts[1].trim().to_string()));

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
            response_line,
            headers,
            initial_body,
        })
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
        let res = TunnelRequest::from_buffer(buffer_bytes);
        assert!(res.is_ok());

        let req = res.expect("TunnelRequest must be created");

        assert_eq!(req.request_line.method.as_str(), "AUTH");
        assert_eq!(req.request_line.path.as_str(), "/auth");
        assert_eq!(req.request_line.version.as_str(), "WEBHOOK/1.0");

        assert_eq!(req.headers.len(), 3);

        let mut headers = req.headers.iter();
        let auth = headers
            .next()
            .expect("Authorization header must be present");
        assert_eq!(auth.0.as_str(), "Authorization");
        assert_eq!(auth.1.as_str(), "token");

        let ua = headers.next().expect("User-Agent header must be present");
        assert_eq!(ua.0.as_str(), "User-Agent");
        assert_eq!(ua.1.as_str(), "rust-testing");

        let foo = headers.next().expect("X-Foo-Bar header must be present");
        assert_eq!(foo.0.as_str(), "X-Foo-Bar");
        assert_eq!(foo.1.as_str(), "baz");
    }

    #[test]
    fn test_read_tunnel_request_with_body() {
        let buffer = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n{}",
            "FORWARD /webhook WEBHOOK/1.0",
            "Authorization: token",
            "User-Agent: rust-testing",
            "X-Foo-Bar:baz",
            "",
            "POST /webhook HTTP/1.1",
            "Content-Type: application/json",
            "Content-Length: 13",
            "",
            "{\"foo\":\"bar\"}",
        );

        let buffer_bytes = buffer.as_bytes();
        let res = TunnelRequest::from_buffer(buffer_bytes);
        assert!(res.is_ok());

        let req = res.expect("TunnelRequest must be created");

        assert_eq!(req.request_line.method.as_str(), "FORWARD");
        assert_eq!(req.request_line.path.as_str(), "/webhook");
        assert_eq!(req.request_line.version.as_str(), "WEBHOOK/1.0");

        assert_eq!(req.headers.len(), 3);

        let mut headers = req.headers.iter();
        let auth = headers
            .next()
            .expect("Authorization header must be present");
        assert_eq!(auth.0.as_str(), "Authorization");
        assert_eq!(auth.1.as_str(), "token");

        let ua = headers.next().expect("User-Agent header must be present");
        assert_eq!(ua.0.as_str(), "User-Agent");
        assert_eq!(ua.1.as_str(), "rust-testing");

        let foo = headers.next().expect("X-Foo-Bar header must be present");
        assert_eq!(foo.0.as_str(), "X-Foo-Bar");
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
        let res = TunnelRequest::from_buffer(buffer_bytes);
        assert!(res.is_ok());

        let req = res.expect("TunnelRequest must be created");

        assert_eq!(req.request_line.method.as_str(), "POST");
        assert_eq!(req.request_line.path.as_str(), "/webhook");
        assert_eq!(req.request_line.version.as_str(), "HTTP/1.1");

        assert_eq!(req.headers.len(), 2);

        let mut headers = req.headers.iter();
        let auth = headers.next().expect("Content type header must be present");
        assert_eq!(auth.0.as_str(), "Content-Type");
        assert_eq!(auth.1.as_str(), "application/json");

        let ua = headers
            .next()
            .expect("Content length header must be present");
        assert_eq!(ua.0.as_str(), "Content-Length");
        assert_eq!(ua.1.as_str(), "13");

        assert_eq!(req.initial_body.len(), 13);
    }
}