#[cfg(test)]
use std::io::{Read, Write};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use aes::{Aes128, cipher::KeyIvInit};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

type Encryptor = cfb8::Encryptor<Aes128>;
type Decryptor = cfb8::Decryptor<Aes128>;

pub(crate) struct CryptoStream<S> {
    pub(super) inner: S,
    encryptor: Option<Encryptor>,
    decryptor: Option<Decryptor>,
}

impl<S> CryptoStream<S> {
    pub(crate) fn new(inner: S) -> Self {
        Self {
            inner,
            encryptor: None,
            decryptor: None,
        }
    }

    pub(crate) fn encrypt(&mut self, key: &[u8; 16]) {
        self.encryptor = Some(Encryptor::new(key.into(), key.into()));
    }

    pub(crate) fn decrypt(&mut self, key: &[u8; 16]) {
        self.decryptor = Some(Decryptor::new(key.into(), key.into()));
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for CryptoStream<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result
            && let Some(cipher) = &mut self.decryptor
        {
            cipher.decrypt(&mut buf.filled_mut()[before..]);
        }
        result
    }
}

impl<S: AsyncWrite + Unpin> CryptoStream<S> {
    pub(crate) async fn write_all(&mut self, bytes: &mut [u8]) -> io::Result<()> {
        if let Some(cipher) = &mut self.encryptor {
            cipher.encrypt(bytes);
        }
        // Encrypt once; write_all handles Pending and partial writes without advancing the cipher again.
        self.inner.write_all(bytes).await
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
