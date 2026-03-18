use bytemuck::{Pod, Zeroable};
use socket2::{Domain, Protocol, Socket, Type};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};

use crate::compression::CompressionKind;

pub const FRAME_REQUEST: u8 = 0x88;
pub const FRAME_MAGIC: [u8; 4] = [0x33, 0x34, 0x35, 0x36];
pub const DEFAULT_ADDR: &str = "127.0.0.1:8082";
const MAX_CHUNK_LEN: usize = u16::MAX as usize;

//deprecated
#[repr(C)] // Ensures predictable field order
#[derive(Copy, Clone, Pod, Zeroable)] // Pod allows casting to &[u8]
pub struct FrameHeader {
    pub magic: [u8; 4],   // 4 bytes
    pub width: u32,       // 4 bytes
    pub height: u32,      // 4 bytes
    pub pixel_count: u32, // 4 bytes
    pub payload_len: u32, // 4 bytes
    pub compression: u32, // 4 bytes
    pub server_fps: u32,   // 4 bytes
} // Total: 24 bytes

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportMode {
    Tcp,
    Udp,
    Both,
}

impl TransportMode {
    pub fn from_env() -> Self {
        let raw = crate::config::transport();
        Self::from_str(&raw).unwrap_or(Self::Tcp)
    }

    pub fn from_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "tcp" => Some(Self::Tcp),
            "udp" => Some(Self::Udp),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

pub trait ClientConnection {
    fn request_frame(&mut self) -> io::Result<()>;
    fn read_frame_header(&mut self) -> io::Result<FrameHeader>;
    fn read_chunk(&mut self, buffer: &mut [u8]) -> io::Result<usize>;
}

/* pub fn create_client_connection(
    mode: TransportMode,
    addr: &str,
) -> io::Result<Box<dyn ClientConnection>> {
    match mode {
        TransportMode::Tcp => Ok(Box::new(TcpClientConnection::connect(addr)?)),
        TransportMode::Udp => Ok(Box::new(UdpClientConnection::connect(addr)?)),
        TransportMode::Both => match TcpClientConnection::connect(addr) {
            Ok(conn) => Ok(Box::new(conn)),
            Err(_) => Ok(Box::new(UdpClientConnection::connect(addr)?)),
        },
    }
} */

struct TcpClientConnection {
    stream: TcpStream,
}
/*
impl TcpClientConnection {
    fn connect(addr: &str) -> io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Self { stream })
    }
}

impl ClientConnection for TcpClientConnection {
    fn request_frame(&mut self) -> io::Result<()> {
        self.stream.write_all(&[FRAME_REQUEST])
    }

    fn read_frame_header(&mut self) -> io::Result<FrameHeader> {
        let mut buf = [0u8; 24];
        socket.recv_from(&mut buf)?;

        if buf[0..4] != FRAME_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid TCP frame header",
            ));
        }

        return Ok(bytemuck::from_bytes(&buf));
    }

    /*     fn read_frame_header(&mut self) -> io::Result<FrameHeader> {
           let mut magic = [0_u8; 4];
           self.stream.read_exact(&mut magic)?;
           if magic != FRAME_MAGIC {
               return Err(io::Error::new(
                   io::ErrorKind::InvalidData,
                   format!(
                       "invalid frame header: got {:02x?}, expected {:02x?}",
                       magic, FRAME_MAGIC
                   ),
               ));
           }

           let mut w_buffer = [0_u8; 4];
           let mut h_buffer = [0_u8; 4];
           let mut pixel_count_buffer = [0_u8; 4];
           let mut payload_len_buffer = [0_u8; 4];
           let mut compression_buffer = [0_u8; 4];
           self.stream.read_exact(&mut w_buffer)?;
           self.stream.read_exact(&mut h_buffer)?;
           self.stream.read_exact(&mut pixel_count_buffer)?;
           self.stream.read_exact(&mut payload_len_buffer)?;
           self.stream.read_exact(&mut compression_buffer)?;

           let compression_raw = u32::from_le_bytes(compression_buffer);
           let compression = CompressionKind::from_id(compression_raw).ok_or_else(|| {
               io::Error::new(
                   io::ErrorKind::InvalidData,
                   format!("unknown compression codec id: {}", compression_raw),
               )
           })?;

           Ok(FrameHeader {
               width: u32::from_le_bytes(w_buffer),
               height: u32::from_le_bytes(h_buffer),
               pixel_count: u32::from_le_bytes(pixel_count_buffer),
               payload_len: u32::from_le_bytes(payload_len_buffer),
               compression,
           })
       }
    */
    fn read_chunk(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut len_buffer = [0_u8; 2];
        self.stream.read_exact(&mut len_buffer)?;
        let chunk_len = u16::from_le_bytes(len_buffer) as usize;

        if chunk_len > buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incoming chunk is larger than client buffer",
            ));
        }

        self.stream.read_exact(&mut buffer[..chunk_len])?;
        Ok(chunk_len)
    }
}
 */
pub struct UdpClientConnection {
    pub socket: UdpSocket,
}

impl UdpClientConnection {
    pub fn connect(addr: &str) -> io::Result<Self> {
        // Instead of UdpSocket::bind, use socket2 to configure the buffer
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_recv_buffer_size(30 * 1024 * 1024)?; // 30MB buffer
        let connection_socket: std::net::UdpSocket = socket.into();

        //let socket = UdpSocket::bind("0.0.0.0:0")?;
        //socket.connect(addr)?;

        connection_socket.connect(addr)?;
        Ok(Self {
            socket: connection_socket,
        })
    }
}

impl ClientConnection for UdpClientConnection {
    fn request_frame(&mut self) -> io::Result<()> {
        self.socket.send(&[FRAME_REQUEST])?;
        Ok(())
    }

    fn read_frame_header(&mut self) -> io::Result<FrameHeader> {
        let mut packet = [0_u8; std::mem::size_of::<FrameHeader>()];
        let size = self.socket.recv(&mut packet)?;
        if size < packet.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short UDP frame header",
            ));
        }

        if packet[0..4] != FRAME_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid UDP frame header",
            ));
        }

        return Ok(*bytemuck::from_bytes(&packet));
    }

    fn read_chunk(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.socket.recv(buffer)
    }
}

pub trait ServerConnection {
    fn wait_for_frame_request(&mut self) -> io::Result<()>;
    fn send_frame_header(
        &mut self,
        w: u32,
        h: u32,
        pixel_count: u32,
        payload_len: u32,
        compression: CompressionKind,
        server_fps: u32,
    ) -> io::Result<()>;
    fn send_chunk(&mut self, chunk: &[u8]) -> io::Result<()>;
    fn peer_label(&self) -> String;
}

pub struct TcpServerConnection {
    stream: TcpStream,
}

impl TcpServerConnection {
    pub fn new(stream: TcpStream) -> Self {
        stream.set_nodelay(true).ok();
        Self { stream }
    }
}

impl ServerConnection for TcpServerConnection {
    fn wait_for_frame_request(&mut self) -> io::Result<()> {
        let mut buffer = [0_u8; 1];
        self.stream.read_exact(&mut buffer)?;
        if buffer[0] != FRAME_REQUEST {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid frame request marker",
            ));
        }
        Ok(())
    }

    fn send_frame_header(
        &mut self,
        w: u32,
        h: u32,
        pixel_count: u32,
        payload_len: u32,
        compression: CompressionKind,
        server_fps: u32,
    ) -> io::Result<()> {
        let mut buf = [0u8; 24];
        buf[0..4].copy_from_slice(&FRAME_MAGIC);
        buf[4..8].copy_from_slice(&w.to_le_bytes());
        buf[8..12].copy_from_slice(&h.to_le_bytes());
        buf[12..16].copy_from_slice(&pixel_count.to_le_bytes());
        buf[16..20].copy_from_slice(&payload_len.to_le_bytes());
        buf[20..24].copy_from_slice(&compression.id().to_le_bytes());
        self.stream.write_all(&buf)
    }

    fn send_chunk(&mut self, chunk: &[u8]) -> io::Result<()> {
        if chunk.len() > MAX_CHUNK_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "chunk too large for TCP frame protocol",
            ));
        }

        let len = chunk.len() as u16;
        let mut buf = Vec::with_capacity(2 + chunk.len());
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(chunk);
        self.stream.write_all(&buf)
    }

    fn peer_label(&self) -> String {
        match self.stream.peer_addr() {
            Ok(addr) => addr.to_string(),
            Err(_) => "unknown-peer".to_string(),
        }
    }
}

pub struct UdpServerConnection {
    pub socket: UdpSocket,
    pub peer: Option<SocketAddr>,
}

impl UdpServerConnection {
    pub fn bind(addr: &str) -> io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        Ok(Self { socket, peer: None })
    }
}

impl ServerConnection for UdpServerConnection {
    fn wait_for_frame_request(&mut self) -> io::Result<()> {
        let mut buffer = [0_u8; 1];
        loop {
            let (size, addr) = self.socket.recv_from(&mut buffer)?;
            if size == 1 && buffer[0] == FRAME_REQUEST {
                self.peer = Some(addr);
                return Ok(());
            }
        }
    }

    fn send_frame_header(
        &mut self,
        w: u32,
        h: u32,
        pixel_count: u32,
        payload_len: u32,
        compression: CompressionKind,
        server_fps: u32,
    ) -> io::Result<()> {
        let header = FrameHeader {
            magic: FRAME_MAGIC,
            width: w,
            height: h,
            pixel_count,
            payload_len,
            compression: compression.id(),
            server_fps, // Include the server FPS in the header
        };
        let packet = bytemuck::bytes_of(&header);
        self.socket.send_to(
            packet,
            self.peer
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "missing UDP peer"))?,
        )?;
        Ok(())
    }

    fn send_chunk(&mut self, chunk: &[u8]) -> io::Result<()> {
        let peer = self
            .peer
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "missing UDP peer"))?;
        self.socket.send_to(chunk, peer)?;
        Ok(())
    }

    fn peer_label(&self) -> String {
        match self.peer {
            Some(addr) => addr.to_string(),
            None => "unknown-peer".to_string(),
        }
    }
}

pub fn bind_tcp_listener(addr: &str) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}
