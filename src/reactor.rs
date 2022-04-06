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

use std::{
    cell::RefCell,
    os::unix::prelude::RawFd,
    rc::Rc,
    task::{Context, Waker},
};

use polling::{Event, Poller};
use rustc_hash::FxHashMap;

pub(crate) fn get_reactor() -> Rc<RefCell<Reactor>> {
    crate::executor::EX.with(|ex| ex.reactor.clone())
}

#[derive(Debug)]
pub struct Reactor {
    poller: Poller,
    waker_mapping: FxHashMap<u64, Waker>,
    buffer: Vec<Event>,
}

impl Default for Reactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Reactor {
    pub fn new() -> Self {
        Self {
            poller: Poller::new().unwrap(),
            waker_mapping: Default::default(),

            buffer: Vec::with_capacity(2048),
        }
    }

    pub fn add(&mut self, fd: RawFd) {
        let flags =
            nix::fcntl::OFlag::from_bits(nix::fcntl::fcntl(fd, nix::fcntl::F_GETFL).unwrap())
                .unwrap();
        let flags_nonblocking = flags | nix::fcntl::OFlag::O_NONBLOCK;
        nix::fcntl::fcntl(fd, nix::fcntl::F_SETFL(flags_nonblocking)).unwrap();
        self.poller
            .add(fd, polling::Event::none(fd as usize))
            .unwrap();
    }

    pub fn modify_readable(&mut self, fd: RawFd, cx: &mut Context) {
        self.push_completion(fd as u64 * 2, cx);
        let event = polling::Event::readable(fd as usize);
        self.poller.modify(fd, event);
    }

    pub fn modify_writable(&mut self, fd: RawFd, cx: &mut Context) {
        self.push_completion(fd as u64 * 2 + 1, cx);
        let event = polling::Event::writable(fd as usize);
        self.poller.modify(fd, event);
    }

    pub fn wait(&mut self) {
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

    pub fn delete(&mut self, fd: RawFd) {
        self.waker_mapping.remove(&(fd as u64 * 2));
        self.waker_mapping.remove(&(fd as u64 * 2 + 1));
    }

    fn push_completion(&mut self, token: u64, cx: &mut Context) {
        self.waker_mapping.insert(token, cx.waker().clone());
    }
}
