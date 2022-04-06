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

mod error;
mod executor;
mod reactor;
mod rt;
mod tcp;

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::{executor::Executor, tcp::TcpListener};

    #[test]
    fn it_works() {
        let ex = Executor::new();
        ex.block_on(s);
    }

    async fn s() {
        let mut listener = TcpListener::bind("127.0.0.1:30000").unwrap();
        while let Some(ret) = listener.next().await {
            if let Ok((mut stream, addr)) = ret {
                println!("accept a new connection from {} successfully", addr);
                let f = async move {
                    let mut buf = [0; 4096];
                    loop {
                        match stream.read(&mut buf).await {
                            Ok(n) => {
                                if n == 0 || stream.write_all(&buf[..n]).await.is_err() {
                                    return;
                                }
                            }
                            Err(_) => {
                                return;
                            }
                        }
                    }
                };
                Executor::spawn(f);
            }
        }
    }
}
