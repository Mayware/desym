use std::io::{self, BufRead, Write};

use tokio::sync::{Mutex, mpsc, oneshot};

type Callback = oneshot::Sender<String>;

pub struct Input {
    input_tx: mpsc::Sender<Callback>,
    lock: Mutex<()>,
}

// Recommended as per docs: https://docs.rs/tokio/latest/tokio/io/struct.Stdin.html
impl Input {
    pub fn new() -> Self {
        // This background thread listens for input on the mpsc channel. The input gives a tx
        // callback we can fire, which then returns the input
        let (input_tx, mut input_rx) = mpsc::channel::<Callback>(32);
        std::thread::spawn(move || {
            let stdin = io::stdin();
            while let Some(reply) = input_rx.blocking_recv() {
                let mut line = String::new();
                stdin.lock().read_line(&mut line).unwrap();
                let line = line.trim_end_matches(['\n', '\r']).to_string();
                let _ = reply.send(line);
            }
        });

        Self { input_tx, lock: Mutex::new(()) }
    }

    pub async fn println(&self, message: &str) {
        let _guard = self.lock.lock().await;
        println!("{}", message);
    }

    pub async fn prompt(&self, prompt: &str) -> String {
        let _guard = self.lock.lock().await;
        print!("{}", prompt);
        io::stdout().flush().unwrap();

        let (tx, rx) = oneshot::channel();
        self.input_tx.send(tx).await.unwrap();
        rx.await.unwrap()
    }
}
