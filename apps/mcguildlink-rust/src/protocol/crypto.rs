#[cfg(test)]
use std::io::{self, Read, Write};

use aes::{Aes128, cipher::KeyIvInit};

type Encryptor = cfb8::Encryptor<Aes128>;
type Decryptor = cfb8::Decryptor<Aes128>;

pub(crate) struct StreamEncryptor(Encryptor);

impl StreamEncryptor {
    pub(crate) fn new(key: &[u8; 16]) -> Self {
        Self(Encryptor::new(key.into(), key.into()))
    }

    pub(crate) fn apply(&mut self, bytes: &mut [u8]) {
        self.0.encrypt(bytes);
    }
}

pub(crate) struct StreamDecryptor(Decryptor);

impl StreamDecryptor {
    pub(crate) fn new(key: &[u8; 16]) -> Self {
        Self(Decryptor::new(key.into(), key.into()))
    }

    pub(crate) fn apply(&mut self, bytes: &mut [u8]) {
        self.0.decrypt(bytes);
    }
}

#[cfg(test)]
pub(crate) struct Cfb8Reader<R> {
    inner: R,
    decryptor: Decryptor,
}

#[cfg(test)]
impl<R> Cfb8Reader<R> {
    pub(crate) fn new(inner: R, key: &[u8; 16]) -> Self {
        Self {
            inner,
            decryptor: Decryptor::new(key.into(), key.into()),
        }
    }
}

#[cfg(test)]
impl<R: Read> Read for Cfb8Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.decryptor.decrypt(&mut buffer[..count]);
        Ok(count)
    }
}

#[cfg(test)]
pub(crate) struct Cfb8Writer<W> {
    inner: W,
    encryptor: Encryptor,
}

#[cfg(test)]
impl<W> Cfb8Writer<W> {
    pub(crate) fn new(inner: W, key: &[u8; 16]) -> Self {
        Self {
            inner,
            encryptor: Encryptor::new(key.into(), key.into()),
        }
    }
}

#[cfg(test)]
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
