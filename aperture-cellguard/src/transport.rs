//! COBS-framed packet exchange over a [`BusLink`].
//!
//! This mirrors the transport of `cellguard-cli` in async form: send one
//! packet, then await one complete reply frame. Framing and CRC checks come
//! from `cellguard-protocol`, so nothing on the wire is reinvented here.

use std::io;
use std::time::Duration;

use cellguard_protocol::{
    DecodeError, Decoder, Error as PacketError, Kind, Packet, encode_frame, max_encoded_len,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

use crate::link::BusLink;

/// Decoded-frame budget for replies. The largest polled reply is a cell
/// snapshot (17 B payload), so 256 leaves generous headroom.
const RX_RAW: usize = 256;

/// Raw-frame budget for requests. Every polled request has an empty
/// payload.
const TX_RAW: usize = 256;

const TX_WIRE: usize = max_encoded_len(TX_RAW);

/// A decoded reply packet with an owned payload copy.
#[derive(Debug, Clone)]
pub struct Reply {
    /// Message kind.
    pub kind: Kind,
    /// Payload bytes.
    pub payload: Vec<u8>,
}

/// One exchange attempt failed.
#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    /// The link is broken. The caller must drop it and reopen.
    #[error("bus link io failed")]
    Io(#[from] io::Error),
    /// No complete reply frame arrived within the reply timeout.
    #[error("no reply within the reply timeout")]
    Timeout,
    /// The reply did not decode as a COBS frame.
    #[error("reply failed to decode as a COBS frame")]
    Decode(DecodeError),
    /// The decoded frame is not a valid packet.
    #[error("reply failed to parse as a packet")]
    Parse(#[from] PacketError),
}

/// A [`BusLink`] turned into request/reply packet frames.
pub struct Framed<L> {
    link: L,
    decoder: Decoder,
    rx: [u8; RX_RAW],
}

impl<L: BusLink> Framed<L> {
    /// Wraps an open link.
    pub(crate) const fn new(link: L) -> Self {
        Self {
            link,
            decoder: Decoder::new(),
            rx: [0; RX_RAW],
        }
    }

    /// Sends one request packet addressed to `id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the link write fails.
    pub(crate) async fn send(&mut self, id: u8, kind: Kind, payload: &[u8]) -> io::Result<()> {
        let mut raw = [0; TX_RAW];
        let raw_len = Packet::write(id, kind, payload, &mut raw).map_err(io::Error::other)?;
        let mut wire = [0; TX_WIRE];
        let wire_len = encode_frame(&raw[..raw_len], &mut wire).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "request frame exceeds tx buffer",
            )
        })?;
        self.link.write_all(&wire[..wire_len]).await?;
        self.link.flush().await
    }

    /// Awaits the next complete reply frame.
    ///
    /// A partial frame left over from an earlier timeout keeps consuming
    /// bytes: COBS resynchronizes at the next frame delimiter.
    ///
    /// # Errors
    ///
    /// Returns [`ExchangeError::Timeout`] when no frame completes within
    /// `timeout_dur`, and [`ExchangeError::Io`] when the link fails.
    pub(crate) async fn recv(&mut self, timeout_dur: Duration) -> Result<Reply, ExchangeError> {
        let read = timeout(timeout_dur, async {
            loop {
                let mut byte = [0; 1];
                let read = self.link.read(&mut byte).await?;
                if read == 0 {
                    return Err(ExchangeError::Io(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "bus link closed",
                    )));
                }
                match self.decoder.feed(byte[0], &mut self.rx) {
                    Ok(Some(len)) => {
                        let Some(frame) = self.rx.get(..len) else {
                            return Err(ExchangeError::Decode(DecodeError::BufferTooSmall));
                        };
                        let packet = Packet::parse(frame)?;
                        return Ok(Reply {
                            kind: packet.kind,
                            payload: packet.payload.to_vec(),
                        });
                    }
                    Ok(None) => {}
                    Err(err) => return Err(ExchangeError::Decode(err)),
                }
            }
        })
        .await;
        match read {
            Ok(result) => result,
            Err(_) => Err(ExchangeError::Timeout),
        }
    }
}

#[cfg(test)]
mod tests {
    use cellguard_protocol::Decoder;
    use tokio::io::duplex;

    use super::*;

    /// Encodes a request packet into a complete COBS frame.
    fn wire_frame(id: u8, kind: Kind, payload: &[u8]) -> Vec<u8> {
        let mut raw = [0; TX_RAW];
        let raw_len = Packet::write(id, kind, payload, &mut raw).expect("packet fits");
        let mut wire = [0; TX_WIRE];
        let wire_len = encode_frame(&raw[..raw_len], &mut wire).expect("wire fits");
        wire[..wire_len].to_vec()
    }

    /// Decodes the first complete COBS frame from `wire`.
    fn decode_first(wire: &[u8]) -> Vec<u8> {
        let mut decoder = Decoder::new();
        let mut buf = [0; RX_RAW];
        for &byte in wire {
            if let Some(len) = decoder.feed(byte, &mut buf).expect("decodes") {
                return buf[..len].to_vec();
            }
        }
        panic!("no complete frame in the wire bytes");
    }

    #[tokio::test]
    async fn send_produces_a_decodable_frame() {
        let (driver_side, mut device_side) = duplex(1024);
        let mut framed = Framed::new(driver_side);
        framed.send(5, Kind::ReadRails, &[]).await.unwrap();

        let mut byte = [0; 1];
        let mut wire = Vec::new();
        while device_side.read(&mut byte).await.unwrap() == 1 {
            wire.push(byte[0]);
            if wire.last() == Some(&0) {
                break;
            }
        }
        let frame = decode_first(&wire);
        let packet = Packet::parse(&frame).unwrap();
        assert_eq!(packet.id, 5);
        assert_eq!(packet.kind, Kind::ReadRails);
        assert!(packet.payload.is_empty());
    }

    #[tokio::test]
    async fn recv_decodes_a_reply() {
        let (driver_side, mut device_side) = duplex(1024);
        device_side
            .write_all(&wire_frame(
                1,
                Kind::BalancerStatus,
                &[1, 2, 3, 4, 5, 6, 7, 8],
            ))
            .await
            .unwrap();

        let mut framed = Framed::new(driver_side);
        let reply = framed.recv(Duration::from_secs(1)).await.unwrap();
        assert_eq!(reply.kind, Kind::BalancerStatus);
        assert_eq!(reply.payload, [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[tokio::test]
    async fn recv_times_out_without_a_reply() {
        let (driver_side, _device_side) = duplex(1024);
        let mut framed = Framed::new(driver_side);
        let err = framed.recv(Duration::from_millis(20)).await.unwrap_err();
        assert!(matches!(err, ExchangeError::Timeout));
    }

    #[tokio::test]
    async fn recv_rejects_a_corrupt_frame() {
        let (driver_side, mut device_side) = duplex(1024);
        let mut wire = wire_frame(1, Kind::CellVoltages, &[9; 17]);
        let last_payload_index = wire.len() - 4;
        wire[last_payload_index] ^= 0x55;
        device_side.write_all(&wire).await.unwrap();

        let mut framed = Framed::new(driver_side);
        let err = framed.recv(Duration::from_millis(100)).await.unwrap_err();
        assert!(matches!(err, ExchangeError::Parse(_)));
    }

    #[tokio::test]
    async fn recv_recovers_after_a_partial_frame() {
        let (driver_side, mut device_side) = duplex(1024);
        // A truncated frame merges with the next one (their delimiters are
        // gone or consumed), so both are lost at the next delimiter. The
        // reply after that decodes cleanly again.
        let mut wire = wire_frame(1, Kind::Rails, &[1; 16]);
        wire.truncate(wire.len() - 2);
        wire.extend_from_slice(&wire_frame(1, Kind::Rails, &[3; 16]));
        wire.extend_from_slice(&wire_frame(1, Kind::Rails, &[2; 16]));
        device_side.write_all(&wire).await.unwrap();

        let mut framed = Framed::new(driver_side);
        let first = framed.recv(Duration::from_millis(10)).await;
        assert!(first.is_err(), "the merged frames must not decode");
        let reply = framed.recv(Duration::from_millis(100)).await.unwrap();
        assert_eq!(reply.kind, Kind::Rails);
        assert_eq!(reply.payload, [2; 16]);
    }
}
