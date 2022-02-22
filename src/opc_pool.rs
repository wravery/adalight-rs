use std::{
    io::{Result, Write},
    net::{Shutdown, TcpStream},
};

use crate::{
    pixel_buffer::PixelBuffer,
    settings::{OpcServer, Settings},
};

struct OpcConnection<'a> {
    server: &'a OpcServer,
    stream: Option<TcpStream>,
}

impl<'a> OpcConnection<'a> {
    pub fn new(server: &'a OpcServer) -> Self {
        Self {
            server,
            stream: None,
        }
    }

    pub fn open(&mut self) -> Result<()> {
        let stream = TcpStream::connect(format!("{}:{}", self.server.host, self.server.port))?;
        stream.shutdown(Shutdown::Read)?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn send(&mut self, pixels: &PixelBuffer) -> bool {
        match self.stream.as_mut() {
            Some(stream) => match stream.write_all(pixels.data()) {
                Ok(()) => true,
                Err(_) => {
                    self.close();
                    false
                }
            },
            None => false,
        }
    }

    pub fn close(&mut self) {
        let _ = match self.stream.take() {
            Some(stream) => stream.shutdown(Shutdown::Both),
            None => Ok(()),
        };
    }
}

pub struct OpcPool<'a> {
    parameters: &'a Settings,
    connections: Vec<OpcConnection<'a>>,
}

impl<'a> OpcPool<'a> {
    pub fn new(parameters: &'a Settings) -> Self {
        Self {
            parameters,
            connections: Vec::new(),
        }
    }

    pub fn open(&mut self) -> bool {
        if self.connections.is_empty() {
            self.connections
                .reserve_exact(self.parameters.servers.len());
            for server in self.parameters.servers.iter() {
                self.connections.push(OpcConnection::new(server));
            }
        }

        let mut opened = false;

        for connection in self.connections.iter_mut() {
            if connection.open().is_ok() {
                opened = true;
            }
        }

        opened
    }

    pub fn send(&mut self, server: usize, pixels: &PixelBuffer) -> bool {
        server < self.connections.len() && self.connections[server].send(pixels)
    }

    pub fn close(&mut self) {
        for connection in self.connections.iter_mut() {
            connection.close();
        }
    }
}

impl<'a> Drop for OpcPool<'a> {
    fn drop(&mut self) {
        self.close();
    }
}
