// Copyright 2022 The Engula Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{cell::RefCell, io, os::unix::prelude::RawFd, rc::Rc, result::Result, task::Context};

#[derive(Debug)]
pub enum IoMode {
    Epoll,
    IoUring,
}

#[derive(Debug)]
pub struct IoContext<T>
where
    T: IoService,
{
    svc: Rc<RefCell<T>>,
}

impl<T> IoContext<T>
where
    T: IoService,
{
    pub fn new(svc: T) -> Self {
        Self {
            svc: Rc::new(RefCell::new(svc)),
        }
    }

    pub fn get_io_service(&self) -> Rc<RefCell<T>> {
        self.svc.clone()
    }
}

impl<T> Clone for IoContext<T>
where
    T: IoService,
{
    fn clone(&self) -> Self {
        Self {
            svc: self.svc.clone(),
        }
    }
}

pub trait IoService {
    fn register_fd(&mut self, fd: RawFd) -> Result<(), io::Error>;
    fn io_mode(&self) -> IoMode;
    fn modify_readable(&mut self, fd: RawFd, cx: &mut Context<'_>);
    fn unregister_fd(&mut self, fd: RawFd);
    fn modify_writable(&mut self, fd: RawFd, cx: &mut Context);
    fn wait(&mut self);
}

pub mod pull {
    use super::IoService;
    use polling::Event;
    use polling::Poller;
    use rustc_hash::FxHashMap;
    use std::{
        io,
        os::unix::prelude::RawFd,
        task::{Context, Waker},
    };

    pub struct EpollReactor {
        poller: Poller,
        waker_mapping: FxHashMap<u64, Waker>,
        buffer: Vec<Event>,
    }

    impl EpollReactor {
        pub fn new() -> Self {
            Self {
                poller: Poller::new().unwrap(),
                waker_mapping: Default::default(),
                buffer: Vec::with_capacity(2048),
            }
        }

        fn push_completion(&mut self, token: u64, cx: &mut Context) {
            self.waker_mapping.insert(token, cx.waker().clone());
        }
    }

    impl IoService for EpollReactor {
        fn register_fd(&mut self, fd: RawFd) -> Result<(), io::Error> {
            let flags =
                nix::fcntl::OFlag::from_bits(nix::fcntl::fcntl(fd, nix::fcntl::F_GETFL).unwrap())
                    .unwrap();
            let flags_nonblocking = flags | nix::fcntl::OFlag::O_NONBLOCK;
            nix::fcntl::fcntl(fd, nix::fcntl::F_SETFL(flags_nonblocking)).unwrap();
            self.poller.add(fd, polling::Event::none(fd as usize))?;
            Ok(())
        }

        fn io_mode(&self) -> super::IoMode {
            super::IoMode::Epoll
        }

        fn modify_readable(&mut self, fd: RawFd, cx: &mut Context<'_>) {
            self.push_completion(fd as u64 * 2, cx);
            let event = polling::Event::readable(fd as usize);
            self.poller.modify(fd, event);
        }

        fn unregister_fd(&mut self, fd: RawFd) {
            self.waker_mapping.remove(&(fd as u64 * 2));
            self.waker_mapping.remove(&(fd as u64 * 2 + 1));
        }

        fn modify_writable(&mut self, fd: RawFd, cx: &mut Context) {
            self.push_completion(fd as u64 * 2 + 1, cx);
            let event = polling::Event::writable(fd as usize);
            self.poller.modify(fd, event);
        }

        fn wait(&mut self) {
            self.poller.wait(&mut self.buffer, None);
            for i in 0..self.buffer.len() {
                let event = self.buffer.swap_remove(0);
                if event.readable {
                    if let Some(waker) = self.waker_mapping.remove(&(event.key as u64 * 2)) {
                        println!(
                            "[reactor token] fd {} read waker token {} removed and woken",
                            event.key,
                            event.key * 2
                        );
                        waker.wake();
                    }
                }
                if event.writable {
                    if let Some(waker) = self.waker_mapping.remove(&(event.key as u64 * 2 + 1)) {
                        println!(
                            "[reactor token] fd {} write waker token {} removed and woken",
                            event.key,
                            event.key * 2 + 1
                        );
                        waker.wake();
                    }
                }
            }
        }
    }
}

pub mod uring {
    use std::{io, os::unix::prelude::RawFd, task::Context};

    use io_uring::IoUring;

    use super::IoService;

    pub struct UringService {
        uring: IoUring,
    }

    impl IoService for UringService {
        fn register_fd(&mut self, fd: RawFd) -> Result<(), io::Error> {
            self.uring.submitter().register_files_update(1, &[fd])?;
            Ok(())
        }

        fn io_mode(&self) -> super::IoMode {
            super::IoMode::IoUring
        }

        fn modify_readable(&mut self, fd: RawFd, cx: &mut std::task::Context<'_>) {
            todo!()
        }

        fn unregister_fd(&mut self, fd: RawFd) {
            todo!()
        }

        fn modify_writable(&mut self, fd: RawFd, cx: &mut Context) {
            todo!()
        }

        fn wait(&mut self) {
            todo!()
        }
    }
}
