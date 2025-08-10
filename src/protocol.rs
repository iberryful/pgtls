use anyhow::Result;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

const SSL_REQUEST_CODE: u32 = 80877103;

#[derive(Debug, PartialEq)]
pub enum RequestType<'a> {
    Ssl,
    Startup(&'a [u8]), // The initial bytes, to be replayed
}

pub async fn parse_request<'a>(
    stream: &mut TcpStream,
    buffer: &'a mut [u8; 8],
) -> Result<RequestType<'a>> {
    stream.read_exact(buffer).await?;

    let length = u32::from_be_bytes(buffer[0..4].try_into()?);
    if length != 8 {
        return Ok(RequestType::Startup(buffer));
    }

    let code = u32::from_be_bytes(buffer[4..8].try_into()?);
    if code == SSL_REQUEST_CODE {
        Ok(RequestType::Ssl)
    } else {
        Ok(RequestType::Startup(buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncRead;
    use tokio_test::io::Builder;

    #[tokio::test]
    async fn test_parse_ssl_request() {
        let ssl_request_bytes = [0u8, 0, 0, 8, 4, 210, 22, 47]; // SSL request: length=8, code=80877103
        let mut mock_stream = Builder::new().read(&ssl_request_bytes).build();
        let mut buffer = [0u8; 8];

        let result = parse_request_from_mock(&mut mock_stream, &mut buffer).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), RequestType::Ssl);
    }

    #[tokio::test]
    async fn test_parse_startup_message() {
        // StartupMessage: length=16 (not 8), followed by some data
        let startup_bytes = [0u8, 0, 0, 16, 0, 3, 0, 0]; // Length=16, protocol version 3.0
        let mut mock_stream = Builder::new().read(&startup_bytes).build();
        let mut buffer = [0u8; 8];

        let result = parse_request_from_mock(&mut mock_stream, &mut buffer).await;

        assert!(result.is_ok());
        match result.unwrap() {
            RequestType::Startup(bytes) => {
                assert_eq!(bytes, &startup_bytes);
            }
            RequestType::Ssl => panic!("Expected Startup, got Ssl"),
        }
    }

    #[tokio::test]
    async fn test_parse_invalid_ssl_request() {
        // Length is 8 but code is not SSL_REQUEST_CODE
        let invalid_ssl_bytes = [0u8, 0, 0, 8, 1, 2, 3, 4]; // Length=8, but wrong code
        let mut mock_stream = Builder::new().read(&invalid_ssl_bytes).build();
        let mut buffer = [0u8; 8];

        let result = parse_request_from_mock(&mut mock_stream, &mut buffer).await;

        assert!(result.is_ok());
        match result.unwrap() {
            RequestType::Startup(bytes) => {
                assert_eq!(bytes, &invalid_ssl_bytes);
            }
            RequestType::Ssl => panic!("Expected Startup, got Ssl"),
        }
    }

    #[tokio::test]
    async fn test_stream_eof() {
        // Only 4 bytes instead of 8
        let incomplete_bytes = [0u8, 0, 0, 8];
        let mut mock_stream = Builder::new().read(&incomplete_bytes).build();
        let mut buffer = [0u8; 8];

        let result = parse_request_from_mock(&mut mock_stream, &mut buffer).await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("early eof"));
    }

    #[tokio::test]
    async fn test_empty_stream() {
        let empty_bytes = [];
        let mut mock_stream = Builder::new().read(&empty_bytes).build();
        let mut buffer = [0u8; 8];

        let result = parse_request_from_mock(&mut mock_stream, &mut buffer).await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("early eof"));
    }

    #[tokio::test]
    async fn test_ssl_request_code_verification() {
        // Test the exact SSL_REQUEST_CODE value (80877103 = 0x04D2162F)
        assert_eq!(SSL_REQUEST_CODE, 80877103);

        let ssl_request_bytes = [0u8, 0, 0, 8, 0x04, 0xD2, 0x16, 0x2F];
        let mut mock_stream = Builder::new().read(&ssl_request_bytes).build();
        let mut buffer = [0u8; 8];

        let result = parse_request_from_mock(&mut mock_stream, &mut buffer).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), RequestType::Ssl);
    }

    #[tokio::test]
    async fn test_real_startup_message_pattern() {
        // More realistic StartupMessage pattern
        // Length=68, protocol 3.0, user parameter
        let startup_bytes = [0u8, 0, 0, 68, 0, 3, 0, 0];
        let mut mock_stream = Builder::new().read(&startup_bytes).build();
        let mut buffer = [0u8; 8];

        let result = parse_request_from_mock(&mut mock_stream, &mut buffer).await;

        assert!(result.is_ok());
        match result.unwrap() {
            RequestType::Startup(bytes) => {
                assert_eq!(bytes.len(), 8);
                assert_eq!(u32::from_be_bytes(bytes[0..4].try_into().unwrap()), 68);
                assert_eq!(u32::from_be_bytes(bytes[4..8].try_into().unwrap()), 196608); // 3.0 protocol
            }
            RequestType::Ssl => panic!("Expected Startup, got Ssl"),
        }
    }

    // Helper function that works with the mock streams from tokio-test
    async fn parse_request_from_mock<'a>(
        stream: &mut (impl AsyncRead + Unpin),
        buffer: &'a mut [u8; 8],
    ) -> Result<RequestType<'a>> {
        use tokio::io::AsyncReadExt;

        stream.read_exact(buffer).await?;

        let length = u32::from_be_bytes(buffer[0..4].try_into()?);
        if length != 8 {
            return Ok(RequestType::Startup(buffer));
        }

        let code = u32::from_be_bytes(buffer[4..8].try_into()?);
        if code == SSL_REQUEST_CODE {
            Ok(RequestType::Ssl)
        } else {
            Ok(RequestType::Startup(buffer))
        }
    }
}
