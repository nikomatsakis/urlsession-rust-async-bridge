// Mocked up version of the URL Session API

mod sys {
    use std::sync::{mpsc::Sender, Arc};

    pub struct URLSession {
        thread: Option<std::thread::JoinHandle<()>>,
        tx: Option<Sender<(String, Arc<dyn Delegate + Send + Sync>)>>,
    }

    pub trait Delegate {
        fn got_data(&self, data: &[u8]);
        fn complete(&self);
    }

    impl URLSession {
        pub fn new() -> Self {
            let (tx, rx) = std::sync::mpsc::channel::<(String, Arc<dyn Delegate + Send + Sync>)>();
            let thread = std::thread::spawn(move || {
                for (path, delegate) in rx {
                    match std::fs::read_to_string(path) {
                        Ok(data) => {
                            let bytes: &[u8] = data.as_ref();
                            let mut i = 0;
                            while i < data.len() {
                                // just for fun, only send 1000 bytes at a time
                                let chunk = (data.len() - i).min(1000);
                                eprintln!("i={i:?} chunk={chunk:?} data.len()={:?}", data.len());
                                delegate.got_data(&bytes[i..i + chunk]);
                                i += chunk;
                            }
                        }
                        Err(_) => { /* ignore for now */ }
                    }

                    delegate.complete();
                }
            });

            URLSession {
                thread: Some(thread),
                tx: Some(tx),
            }
        }

        pub fn start_download(&self, path: &str, delegate: Arc<dyn Delegate + Send + Sync>) {
            self.tx
                .as_ref()
                .unwrap()
                .send((path.to_string(), delegate))
                .unwrap();
        }
    }

    impl Drop for URLSession {
        fn drop(&mut self) {
            std::mem::drop(self.tx.take());
            let _ = self.thread.take().unwrap().join();
        }
    }
}

mod rs {
    use std::{
        future::Future,
        sync::{Arc, Mutex},
        task::{Poll, Waker},
    };

    use crate::sys::{self, URLSession};

    pub fn start_download(session: &URLSession, url: &str) -> impl Future<Output = Vec<u8>> {
        let delegate = Arc::new(Mutex::new(Delegate {
            buffer: vec![],
            complete: false,
            waker: None,
        }));
        session.start_download(url, delegate.clone());

        std::future::poll_fn(move |cx| {
            let mut delegate = delegate.lock().unwrap();
            if delegate.complete {
                return Poll::Ready(delegate.buffer.clone()); // FIXME: ideally you'd use `take`...
            } else {
                delegate.waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
        })
    }

    struct Delegate {
        buffer: Vec<u8>,
        complete: bool,
        waker: Option<Waker>,
    }

    impl sys::Delegate for Mutex<Delegate> {
        fn got_data(&self, data: &[u8]) {
            let mut this = self.lock().unwrap();
            this.buffer.extend(data);
        }

        fn complete(&self) {
            let mut this = self.lock().unwrap();
            assert!(!this.complete);
            this.complete = true;
            if let Some(waker) = this.waker.take() {
                waker.wake();
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let url_session = sys::URLSession::new();
    let data = rs::start_download(&url_session, "README.md").await;
    match String::from_utf8(data) {
        Ok(s) => {
            println!("{s}");
        }
        Err(s) => {
            println!("{s:?}");
        }
    }
}
