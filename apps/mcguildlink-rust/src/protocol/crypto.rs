use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::Duration,
};

use aes::{Aes128, cipher::KeyIvInit};

type Encryptor = cfb8::Encryptor<Aes128>;
type Decryptor = cfb8::Decryptor<Aes128>;

pub struct Cfb8Reader<R> {
    inner: R,
    decryptor: Decryptor,
}

impl<R> Cfb8Reader<R> {
    pub fn new(inner: R, key: &[u8; 16]) -> Self {
        Self {
            inner,
            decryptor: Decryptor::new(key.into(), key.into()),
        }
    }
}

impl Cfb8Reader<TcpStream> {
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.inner.set_read_timeout(timeout)
    }
}

impl<R: Read> Read for Cfb8Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.decryptor.decrypt(&mut buffer[..count]);
        Ok(count)
    }
}

pub struct Cfb8Writer<W> {
    inner: W,
    encryptor: Encryptor,
}

impl<W> Cfb8Writer<W> {
    pub fn new(inner: W, key: &[u8; 16]) -> Self {
        Self {
            inner,
            encryptor: Encryptor::new(key.into(), key.into()),
        }
    }
}

impl<W: Write> Write for Cfb8Writer<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut encrypted = buffer.to_vec();
        self.encryptor.encrypt(&mut encrypted);
        self.inner.write_all(&encrypted)?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
