use std::io;
use std::io::Cursor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionKind {
    None,
    Zstd,
    Lz4,
}

impl CompressionKind {
    pub fn from_env() -> Self {
        let raw = crate::config::compression();
        Self::from_str(&raw).unwrap_or(Self::Zstd)
    }

    pub fn from_id(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::None),
            1 => Some(Self::Zstd),
            2 => Some(Self::Lz4),
            _ => None,
        }
    }

    pub fn from_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" | "raw" => Some(Self::None),
            "zstd" => Some(Self::Zstd),
            "lz4" => Some(Self::Lz4),
            _ => None,
        }
    }

    pub fn id(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Zstd => 1,
            Self::Lz4 => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Zstd => "zstd",
            Self::Lz4 => "lz4",
        }
    }
}

pub trait FrameCompression {
    fn kind(&self) -> CompressionKind;
    fn compress(&self, input: &[u8]) -> io::Result<Vec<u8>>;
    fn decompress(&self, input: &[u8], expected_size: usize) -> io::Result<Vec<u8>>;
}

struct NoCompression;

impl FrameCompression for NoCompression {
    fn kind(&self) -> CompressionKind {
        CompressionKind::None
    }

    fn compress(&self, input: &[u8]) -> io::Result<Vec<u8>> {
        Ok(input.to_vec())
    }

    fn decompress(&self, input: &[u8], expected_size: usize) -> io::Result<Vec<u8>> {
        if input.len() != expected_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "raw frame size mismatch: got {} bytes, expected {}",
                    input.len(),
                    expected_size
                ),
            ));
        }

        Ok(input.to_vec())
    }
}

struct ZstdCompression {
    level: i32,
}

impl FrameCompression for ZstdCompression {
    fn kind(&self) -> CompressionKind {
        CompressionKind::Zstd
    }

    fn compress(&self, input: &[u8]) -> io::Result<Vec<u8>> {
        zstd::stream::encode_all(Cursor::new(input), self.level)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }

    fn decompress(&self, input: &[u8], expected_size: usize) -> io::Result<Vec<u8>> {
        let decoded = zstd::stream::decode_all(Cursor::new(input))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        if decoded.len() != expected_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "zstd frame size mismatch: got {} bytes, expected {}",
                    decoded.len(),
                    expected_size
                ),
            ));
        }

        Ok(decoded)
    }
}

struct Lz4Compression;

impl FrameCompression for Lz4Compression {
    fn kind(&self) -> CompressionKind {
        CompressionKind::Lz4
    }

    fn compress(&self, input: &[u8]) -> io::Result<Vec<u8>> {
        Ok(lz4_flex::block::compress(input))
    }

    fn decompress(&self, input: &[u8], expected_size: usize) -> io::Result<Vec<u8>> {
        lz4_flex::block::decompress(input, expected_size)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }
}

pub fn create_frame_compression(kind: CompressionKind) -> Box<dyn FrameCompression> {
    match kind {
        CompressionKind::None => Box::new(NoCompression),
        CompressionKind::Zstd => Box::new(ZstdCompression { level: 3 }),
        CompressionKind::Lz4 => Box::new(Lz4Compression),
    }
}

pub fn create_frame_compression_from_env() -> Box<dyn FrameCompression> {
    create_frame_compression(CompressionKind::from_env())
}
