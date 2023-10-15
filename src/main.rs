use std::{collections::HashMap, str::FromStr, sync::Arc};

use actix::{Actor, Context, StreamHandler, Addr, AsyncContext, Handler, Message};
use actix_web::{dev::Service, get, web, HttpRequest, HttpResponse, HttpServer, Responder, cookie::time::Time};
use actix_web_actors::ws;
use anyhow::{anyhow, Result};
use async_stream::stream;
use async_trait::async_trait;
use env_logger;
use futures_util::{stream::StreamExt, SinkExt, Stream, TryFutureExt, Future};
use log::{debug, error, warn};
use reqwest::header::HeaderValue;
use serde::Deserialize;
use serde_json::json;
use url::Url;

mod types;
mod timeline_stream;

struct GetTimelineOption {
    until: Option<i32>,
    since: Option<i32>,
}

type SingleChannelReceiver<T> = tokio::sync::mpsc::Receiver<T>;
type MultiChannelReceiver<T> = tokio::sync::broadcast::Receiver<T>;

type SingleChannelSender<T> = tokio::sync::mpsc::Sender<T>;

type Date = chrono::DateTime<chrono::Utc>;

#[derive(Clone, Debug)]
struct Instance {
    name: String,
    softwareName: String,
    iconUrl: String,
    faviconUrl: String,
    themeColor: String,
}

#[derive(Clone, Debug)]
struct User {
    name: String,
    username: String,
    host: String,
    instance: Instance,
}

#[derive(Clone, Debug)]
struct TimelineItem {
    createdAt: Date,
    user: User,
}

type Timeline = Vec<TimelineItem>;

struct TimelineStreamReceiver {}

#[async_trait]
trait StreamingStrategy {
    fn event_receiver(self: Box<Self>) -> SingleChannelReceiver<Timeline>;
    fn get_strategy_id(&self) -> &'static str;
}

struct MisskeyDirectStreaming {
    receiver: SingleChannelReceiver<Timeline>,
}

#[derive(Debug, Deserialize)]
struct NodeInfoSoftware {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct NodeInfo {
    software: NodeInfoSoftware,
}

impl MisskeyDirectStreaming {
    async fn new(mut host: Url) -> Result<Self> {
        // NOTE: Constant literal changes does not returns error.
        host.set_scheme("wss").unwrap();
        let wsurl = host.join("/streaming").unwrap();

        debug!("Connecting to {:?}", host.to_string());
        let (ws_stream, _) = http_client::ws_connect_async(wsurl.clone()).await?;
        let (mut write, mut read) = ws_stream.split();
        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(10);

        write
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({
                    "type": "connect",
                    "body": {
                        "channel": "globalTimeline", // TODO: localTimelineにする
                        "id": "ltl"
                    }
                })
                .to_string(),
            ))
            .await?;

        {
            let wsurl = wsurl.clone();
            tokio::spawn(async move {
                // TODO: heartbeat & kill detection
                debug!("Heartbeat");

                loop {
                    if let Err(e) = write.send("h".into()).await {
                        warn!("Seems disconnected from {}", &wsurl);
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
            });
        }

        tokio::spawn(async move {
            debug!("Receive thread start!");

            while let Some(item) = read.next().await {
                match item {
                    Ok(message) => match message {
                        tokio_tungstenite::tungstenite::Message::Text(text) => {
                            if let Err(_) = event_sender.send(Vec::new()).await {
                                return; // recvが終わってたら終了
                            }

                            let json = serde_json::from_str::<serde_json::value::Value>(&text);

                            match json {
                                Ok(json) => debug!("Received json {}", &json.to_string()),
                                Err(e) => debug!("Received non-json {:?}", &text),
                            }
                        }
                        _ => debug!("Received non-text {:?}", message),
                    },
                    Err(error) => {
                        // TODO: error
                        error!("{:?}", error);
                    }
                }
            }

            debug!("Disconnected from {}", &wsurl);

            // TODO: disconnect
        });

        Ok(Self {
            receiver: event_receiver,
        })
    }
}

mod http_client {
    use std::{sync::Mutex, time::Duration};

    use log::{debug, warn};
    use once_cell::sync::OnceCell;
    use reqwest;
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
                    CIRCUIT_BREAKER_GLOBAL.get().unwrap();
                }

                std::thread::spawn(|| {
                    // NOTE: Block for not to lock instance too long
                    loop {
                        {
                            let mut instance =
                                CIRCUIT_BREAKER_GLOBAL.get().unwrap().lock().unwrap();

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

                return CIRCUIT_BREAKER_GLOBAL.get().unwrap();
            }
        }

        fn readed() {
            let mut instance = Self::global().lock().unwrap();

            instance.read_count_per_sec += 1;
        }
    }

    pub async fn get(url: Url) -> reqwest::Result<reqwest::Response> {
        CircuitBreaker::readed();
        reqwest::get(url).await
    }

    pub async fn ws_connect_async(
        url: Url,
    ) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), Error> {
        tokio_tungstenite::connect_async(url).await
    }
}

enum Strategy {
    MisskeyDirect,
}

async fn get_remote_software(host: &Url) -> Result<Strategy> {
    let nodeinfo_text = http_client::get(host.join("nodeinfo/2.0")?)
        .await?
        .text()
        .await?;

    let nodeinfo_struct = serde_json::from_str::<NodeInfo>(&nodeinfo_text)?;

    match nodeinfo_struct.software.name.as_str() {
        "misskey" => return Ok(Strategy::MisskeyDirect),
        _ => {
            return Err(anyhow!(
                "The software".to_owned() + &nodeinfo_struct.software.name + " is not supported."
            ))
        }
    }
}

async fn get_stream(host: Url) -> Result<Box<dyn StreamingStrategy>> {
    match get_remote_software(&host).await? {
        Strategy::MisskeyDirect => {
            debug!("Host {} seems to be running Misskey.", &host);
            Ok(Box::new(MisskeyDirectStreaming::new(host).await?))
        }
    }
}

async fn misskey_direct_streaming(mut host: Url) -> Result<impl Stream<Item = Timeline>> {
    host.set_scheme("wss").unwrap();
    let wsurl = host.join("/streaming").unwrap();

    debug!("Connecting to {:?}", host.to_string());
    let (mut write, mut read) = http_client::ws_connect_async(wsurl.clone()).await?.0.split();

    write
        .send(tokio_tungstenite::tungstenite::Message::Text(
            json!({
                "type": "connect",
                "body": {
                    "channel": "globalTimeline", // TODO: localTimelineにする
                    "id": "ltl"
                }
            })
            .to_string(),
        ))
        .await?;

    Ok(stream! {
        debug!("Receive thread start!");

        while let Some(item) = read.next().await {
            match item {
                Ok(message) => match message {
                    tokio_tungstenite::tungstenite::Message::Text(text) => {
                        let json = serde_json::from_str::<serde_json::value::Value>(&text);

                        match json {
                            Ok(json) => debug!("Received json {}", &json.to_string()),
                            Err(_) => debug!("Received non-json {:?}", &text),
                        };

                        yield vec![]; // TODO
                    }
                    _ => debug!("Received non-text {:?}", message),
                },
                Err(error) => {
                    // TODO: error
                    error!("{:?}", error);
                }
            };
        }

        debug!("Disconnected from {}", &wsurl);

        yield Vec::new();
    })

    // NOTE: Constant literal changes does not returns error.
    /*

    /*

        /*loop {
            if let Err(e) = write.send("h".into()).await {
                warn!("Seems disconnected from {}", &wsurl);
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }

    Ok(stream!{
        let wsurl = wsurl.clone();
            // TODO: heartbeat & kill detection
            debug!("Heartbeat");

            debug!("Receive thread start!");



        })*/ */ */
}


#[async_trait]
impl StreamingStrategy for MisskeyDirectStreaming {
    fn event_receiver(self: Box<Self>) -> SingleChannelReceiver<Timeline> {
        self.receiver
    }
    fn get_strategy_id(&self) -> &'static str {
        "misskey-direct-connect"
    }
}

async fn authorized(req: &HttpRequest) -> bool {
    // TODO: miauth
    let authorized = req
        .headers()
        .get("Authorized")
        .map(|authorized| authorized.to_str().unwrap_or(""))
        .unwrap_or("");

    return true;
}

struct StreamingClient;
struct StreamingError;
struct AddClientRequest;

impl Actor for StreamingClient {
    type Context = Context<Self>;
}

struct TimelineStreamingActor {
    dest: Vec<Addr<StreamingClient>>
}

impl TimelineStreamingActor {
    pub fn start(timeline_stream: std::pin::Pin<Box<dyn Stream<Item = Timeline>>>) -> Addr<Self> {
        Self::create(|ctx| {
            ctx.add_stream(timeline_stream);
            Self {
                dest: Vec::new()
            }
        })
    }
}

impl Actor for TimelineStreamingActor {
    type Context = actix::Context<Self>;

    fn stopped(&mut self, ctx: &mut Self::Context) {
        
    }
}

impl StreamHandler<Timeline> for TimelineStreamingActor {
    fn handle(&mut self, item: Timeline, ctx: &mut Self::Context) {
        todo!()
    }
}

impl Handler<StreamingError> for TimelineStreamingActor {
    type Result = ();

    fn handle(&mut self, msg: StreamingError, ctx: &mut Self::Context) -> Self::Result {
        todo!()
    }
}

impl Message for StreamingError {
    type Result = ();
}


#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().json(json!({"version": "0.0.0"}))
}

#[get("/stream/{server}")]
async fn stream(req: HttpRequest, path: web::Path<(String,)>) -> impl Responder {
    if !authorized(&req).await {
        return HttpResponse::Unauthorized().into();
    }

    let (server,) = path.into_inner();

    HttpResponse::Ok().json(json!({"requested": server}))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    actix_web::HttpServer::new(|| actix_web::App::new().service(index).service(stream))
        .bind(("127.0.0.1", 8080))?
        .run()
        .await

    /*

    let mut stream = get_stream(Url::from_str("https://superneko.net")?)
        .await
        .unwrap();

    let mut receiver = stream.event_receiver();
    while let Some(message) = receiver.recv().await {
        println!("{:?}", message);
    }

    println!("Hello, world!");


    return Ok(());
    */
}
