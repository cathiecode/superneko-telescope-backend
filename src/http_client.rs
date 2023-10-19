use std::{sync::Mutex, time::Duration};

use log::{debug, warn};
use once_cell::sync::OnceCell;
use reqwest::{self, Body, RequestBuilder};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    tungstenite::{handshake::client::Response, Error},
    MaybeTlsStream, WebSocketStream,
};
use url::Url;

struct CircuitBreaker {
    read_count_per_sec: i32,
    overload_read_count_per_sec: i32,
    overload_trelance_sec: i32,
}

impl CircuitBreaker {
    fn global() -> &'static Mutex<CircuitBreaker> {
        static CIRCUIT_BREAKER_GLOBAL: OnceCell<Mutex<CircuitBreaker>> = OnceCell::new();

        if let Some(instance) = CIRCUIT_BREAKER_GLOBAL.get() {
            return instance;
        } else {
            debug!("Circuit breaker initialized.");
            let result = CIRCUIT_BREAKER_GLOBAL.set(Mutex::new(Self {
                read_count_per_sec: 0,
                overload_read_count_per_sec: 0,

                overload_trelance_sec: 5,
            }));

            if let Err(_) = result {
                CIRCUIT_BREAKER_GLOBAL.get().unwrap(); // NOTE: できなかったらプログラムミス
            }

            std::thread::spawn(|| {
                // NOTE: Block for not to lock instance too long
                loop {
                    {
                        let mut instance =
                            CIRCUIT_BREAKER_GLOBAL.get().unwrap().lock().unwrap(); // NOTE: できなかったらプログラムミス

                        if instance.read_count_per_sec >= instance.overload_read_count_per_sec {
                            instance.overload_read_count_per_sec += 1;
                            instance.read_count_per_sec = 0;

                            warn!("Circuit breaker detects overload! Current overload seconds count is {}", instance.overload_read_count_per_sec);
                        }

                        if instance.overload_read_count_per_sec
                            >= instance.overload_trelance_sec
                        {
                            panic!("Too many requests! breaking circuit!");
                        }
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            });

            return CIRCUIT_BREAKER_GLOBAL.get().unwrap(); // NOTE: できなかったらプログラムミス
        }
    }

    fn readed() {
        let mut instance = Self::global().lock().unwrap(); // NOTE: できなかったらプログラムミス

        instance.read_count_per_sec += 1;
    }
}

pub async fn get(url: Url) -> reqwest::Result<reqwest::Response> {
    CircuitBreaker::readed();
    reqwest::get(url).await
}

pub async fn request(request: RequestBuilder) -> reqwest::Result<reqwest::Response> {
    CircuitBreaker::readed();
    request.send().await
    /*;
    client.post(url).body(body)*/
}

pub async fn ws_connect_async(
    url: Url,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), Error> {
    tokio_tungstenite::connect_async(url).await
}
