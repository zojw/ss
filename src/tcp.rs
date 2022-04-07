use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener as StdTcpListener, TcpStream as StdTcpStream, ToSocketAddrs},
    os::unix::prelude::AsRawFd,
    task::Poll,
};

use futures::Stream;
use socket2::{Domain, Protocol, Socket, Type};

use crate::io_ctx::{self, IoContext, IoMode, IoService};

#[derive(Debug)]
pub struct TcpListener<T: IoService> {
    ctx: IoContext<T>,
    listener: StdTcpListener,
}

impl<T: IoService> TcpListener<T> {
    pub fn bind<A: ToSocketAddrs>(addr: A, ctx: io_ctx::IoContext<T>) -> Result<Self, io::Error> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "empty address"))?;

        let domain = if addr.is_ipv6() {
            Domain::IPV6
        } else {
            Domain::IPV4
        };
        let sk = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        let addr = socket2::SockAddr::from(addr);
        sk.set_reuse_address(true)?;
        sk.bind(&addr)?;
        sk.listen(1024)?;
        (*ctx.get_io_service())
            .borrow_mut()
            .register_fd(sk.as_raw_fd())?;
        Ok(Self {
            ctx,
            listener: sk.into(),
        })
    }
}

impl<T: IoService> Stream for TcpListener<T> {
    type Item = std::io::Result<(TcpStream<T>, SocketAddr)>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let me = self.get_mut();
        let io_svc = me.ctx.get_io_service();
        let mut svc = (*io_svc).borrow_mut();
        match svc.io_mode() {
            IoMode::Epoll => match me.listener.accept() {
                Ok((stream, addr)) => {
                    svc.register_fd(stream.as_raw_fd());
                    Poll::Ready(Some(Ok((TcpStream::new(stream, me.ctx.clone()), addr))))
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    svc.modify_readable(me.listener.as_raw_fd(), cx);
                    Poll::Pending
                }
                Err(e) => std::task::Poll::Ready(Some(Err(e))),
            },
            IoMode::IoUring => unimplemented!(),
        }
    }
}

#[derive(Debug)]
pub struct TcpStream<T: IoService> {
    ctx: IoContext<T>,
    stream: StdTcpStream,
}

impl<T: IoService> TcpStream<T> {
    fn new(stream: StdTcpStream, ctx: IoContext<T>) -> Self {
        Self {
            stream,
            ctx: ctx.clone(),
        }
    }
}

impl<T: IoService> Drop for TcpStream<T> {
    fn drop(&mut self) {
        (*self.ctx.get_io_service())
            .borrow_mut()
            .unregister_fd(self.stream.as_raw_fd());
    }
}

impl<T: IoService> tokio::io::AsyncRead for TcpStream<T> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        let io_svc = me.ctx.get_io_service();
        let mut svc = (*io_svc).borrow_mut();
        match svc.io_mode() {
            IoMode::Epoll => unsafe {
                let b =
                    &mut *(buf.unfilled_mut() as *mut [std::mem::MaybeUninit<u8>] as *mut [u8]);
                match me.stream.read(b) {
                    Ok(n) => {
                        buf.assume_init(n);
                        buf.advance(n);
                        Poll::Ready(Ok(()))
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        svc.modify_readable(me.stream.as_raw_fd(), cx);
                        Poll::Pending
                    }
                    Err(e) => Poll::Ready(Err(e)),
                }
            },
            IoMode::IoUring => todo!(),
        }
    }
}

impl<T: IoService> tokio::io::AsyncWrite for TcpStream<T> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let me = self.get_mut();
        let io_svc = me.ctx.get_io_service();
        let mut svc = (*io_svc).borrow_mut();
        match svc.io_mode() {
            IoMode::Epoll => match me.stream.write(buf) {
                Ok(n) => Poll::Ready(Ok(n)),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    svc.modify_writable(me.stream.as_raw_fd(), cx);
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            },
            IoMode::IoUring => todo!(),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        self.stream.shutdown(std::net::Shutdown::Write)?;
        Poll::Ready(Ok(()))
    }
}
