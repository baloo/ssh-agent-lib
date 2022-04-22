use byteorder::{BigEndian, ReadBytesExt};
use bytes::{BufMut, BytesMut};
use futures::future::FutureResult;
use log::{error, info};
use tokio::codec::{Decoder, Encoder, Framed};
use tokio::net::TcpListener;
use tokio::prelude::*;
use tokio_uds::UnixListener;

use std::error::Error;
use std::fmt::Debug;
use std::mem::size_of;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use super::error::AgentError;
use super::proto::message::Message;
use super::proto::{from_bytes, to_bytes};

struct MessageCodec;

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = AgentError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut bytes = &src[..];

        if bytes.len() < size_of::<u32>() {
            return Ok(None);
        }

        let length = bytes.read_u32::<BigEndian>()? as usize;

        if bytes.len() < length {
            return Ok(None);
        }

        let message: Message = from_bytes(bytes)?;
        src.advance(size_of::<u32>() + length);
        Ok(Some(message))
    }
}

impl Encoder for MessageCodec {
    type Item = Message;
    type Error = AgentError;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = to_bytes(&to_bytes(&item)?)?;
        dst.put(bytes);
        Ok(())
    }
}

macro_rules! handle_clients {
    ($self:ident, $socket:ident) => {{
        info!("Listening; socket = {:?}", $socket);
        let arc_self = Arc::new($self);
        $socket
            .incoming()
            .map_err(|e| error!("Failed to accept socket; error = {:?}", e))
            .for_each(move |socket| {
                let (write, read) = Framed::new(socket, MessageCodec).split();
                let arc_self = arc_self.clone();
                let connection = write
                    .send_all(read.and_then(move |message| {
                        arc_self.handle_async(message).map_err(|e| {
                            error!("Error handling message; error = {:?}", e);
                            AgentError::User
                        })
                    }))
                    .map(|_| ())
                    .map_err(|e| error!("Error while handling message; error = {:?}", e));
                tokio::spawn(connection)
            })
            .map_err(|e| e.into())
    }};
}

pub trait Agent: 'static + Sync + Send + Sized {
    type Error: Debug + Send + Sync;

    fn handle(&self, message: Message) -> Result<Message, Self::Error>;

    fn handle_async(
        &self,
        message: Message,
    ) -> Box<dyn Future<Item = Message, Error = Self::Error> + Send + Sync> {
        Box::new(FutureResult::from(self.handle(message)))
    }

    #[allow(clippy::unit_arg)]
    fn run_listener(self, socket: UnixListener) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(tokio::run(handle_clients!(self, socket)))
    }

    fn run_unix(self, path: impl AsRef<Path>) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.run_listener(UnixListener::bind(path)?)
    }

    #[allow(clippy::unit_arg)]
    fn run_tcp(self, addr: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        let socket = TcpListener::bind(&addr.parse::<SocketAddr>()?)?;
        Ok(tokio::run(handle_clients!(self, socket)))
    }

    #[allow(clippy::unit_arg)]
    fn listen<T>(self, socket: T) -> Result<(), Box<dyn Error + Send + Sync>>
    where
        T: Into<service_binding::Listener>,
    {
        let socket = socket.into();
        match socket {
            service_binding::Listener::Unix(listener) => {
                let listener = UnixListener::from_std(listener, &Default::default())?;
                Ok(tokio::run(handle_clients!(self, listener)))
            }
            service_binding::Listener::Tcp(listener) => {
                let listener = TcpListener::from_std(listener, &Default::default())?;
                Ok(tokio::run(handle_clients!(self, listener)))
            }
        }
    }
}
