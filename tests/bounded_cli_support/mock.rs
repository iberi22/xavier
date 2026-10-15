use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct MockServer {
    pub url: String,
    pub accepted: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl MockServer {
    // None holds accepted sockets open without ever sending a response.
    pub fn new(body: Option<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let accepted = Arc::new(AtomicUsize::new(0));
        let (done, count) = (stop.clone(), accepted.clone());
        let worker = thread::spawn(move || {
            let mut sockets = Vec::new();
            while !done.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        count.fetch_add(1, Ordering::SeqCst);
                        if let Some(body) = &body {
                            socket
                                .set_read_timeout(Some(Duration::from_secs(1)))
                                .unwrap();
                            socket
                                .set_write_timeout(Some(Duration::from_secs(1)))
                                .unwrap();
                            let mut request = Vec::new();
                            let mut byte = [0];
                            while request.len() < 16384 && !request.ends_with(b"\r\n\r\n") {
                                if socket.read(&mut byte).unwrap_or(0) == 0 {
                                    break;
                                }
                                request.push(byte[0]);
                            }
                            assert!(String::from_utf8_lossy(&request).starts_with("GET /health "));
                            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                        } else {
                            sockets.push(socket);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("mock accept: {e}"),
                }
            }
        });
        Self {
            url,
            accepted,
            stop,
            worker: Some(worker),
        }
    }
    pub fn assert_contacted(&self) {
        assert!(
            self.accepted.load(Ordering::SeqCst) > 0,
            "CLI must reach the mock endpoint"
        );
    }
}
impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.expect("mock server failed");
            }
        }
    }
}
