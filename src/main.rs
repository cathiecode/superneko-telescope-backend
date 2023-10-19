

use actix::{Actor, Addr, Context};
use actix_web::{
    get, web, HttpRequest, HttpResponse, Responder,
};
use actix_web_actors::ws;
use anyhow::{anyhow, Result};


use env_logger;

use once_cell::sync::Lazy;

use serde::Deserialize;
use serde_json::json;
use timeline_stream::actors::timeline_streaming::TimelineStreamSupervisorActor;

use url::Url;

use crate::{timeline_stream::actors::timeline_streaming_websocket::TimelineStreamWebsocket, types::Host};

mod http_client;
mod timeline_stream;
mod types;
mod misskey;

struct GetTimelineOption {
    until: Option<i32>,
    since: Option<i32>,
}

type SingleChannelReceiver<T> = tokio::sync::mpsc::Receiver<T>;
type MultiChannelReceiver<T> = tokio::sync::broadcast::Receiver<T>;

type SingleChannelSender<T> = tokio::sync::mpsc::Sender<T>;

type Date = chrono::DateTime<chrono::Utc>;

/*
#[async_trait]
trait StreamingStrategy {
    fn event_receiver(self: Box<Self>) -> SingleChannelReceiver<Post>;
    fn get_strategy_id(&self) -> &'static str;
}
 */

/*struct MisskeyDirectStreaming {
    receiver: SingleChannelReceiver<Post>,
}*/

#[derive(Debug, Deserialize)]
struct NodeInfoSoftware {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct NodeInfo {
    software: NodeInfoSoftware,
}

/*
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
 */

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

/*#[async_trait]
impl StreamingStrategy for MisskeyDirectStreaming {
    fn event_receiver(self: Box<Self>) -> SingleChannelReceiver<Timeline> {
        self.receiver
    }
    fn get_strategy_id(&self) -> &'static str {
        "misskey-direct-connect"
    }
}*/

async fn authorized(req: &HttpRequest) -> bool {
    // TODO: miauth
    let _authorized = req
        .headers()
        .get("Authorized")
        .map(|authorized| authorized.to_str().unwrap_or(""))
        .unwrap_or("");

    return true;
}

struct StreamingClient;
struct StreamingError;

impl Actor for StreamingClient {
    type Context = Context<Self>;
}

struct TimelineStreamingActor {
    dest: Vec<Addr<StreamingClient>>,
}
/*
impl TimelineStreamingActor {
    pub fn start(timeline_stream: std::pin::Pin<Box<dyn Stream<Item = Post>>>) -> Addr<Self> {
        Self::create(|ctx| {
            ctx.add_stream(timeline_stream);
            Self { dest: Vec::new() }
        })
    }
}

impl Actor for TimelineStreamingActor {
    type Context = actix::Context<Self>;

    fn stopped(&mut self, ctx: &mut Self::Context) {}
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
 */

static TIMELINE_STREAM_WEBSOCKET_SUPERVISOR: Lazy<Addr<TimelineStreamSupervisorActor>> =
    Lazy::new(|| TimelineStreamSupervisorActor::new());

#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().message_body(json!({"version": "0.0.0"}).to_string())
}

#[get("/stream/{server}")]
async fn stream(req: HttpRequest, stream: web::Payload, path: web::Path<(String,)>) -> impl Responder {
    if !authorized(&req).await {
        return HttpResponse::Unauthorized().into();
    }

    let (server,) = path.into_inner();

    let host = Host::new(server.parse::<Url>().unwrap());

    ws::start(
        TimelineStreamWebsocket::new(host, TIMELINE_STREAM_WEBSOCKET_SUPERVISOR.clone()),
        &req,
        stream,
    ).unwrap()
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
