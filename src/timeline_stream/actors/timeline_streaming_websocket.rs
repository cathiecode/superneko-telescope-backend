use actix::{Actor, Addr, Handler, StreamHandler, AsyncContext, Message};
use actix_web_actors::ws::{self, WebsocketContext};
use async_trait::async_trait;

use crate::types::Host;

use super::timeline_streaming::{
    RequestTimelineStreamer, TimelineMessage, TimelineStreamSupervisorActor, TimelineStreamerActor, RequestTimelineStream,
};

pub struct TimelineStreamWebsocket {
    stream_supervisor: Addr<TimelineStreamSupervisorActor>,
    stream: Option<Addr<TimelineStreamerActor>>,
    host: Host,
}

impl Actor for TimelineStreamWebsocket {
    type Context = WebsocketContext<Self>;
}

impl TimelineStreamWebsocket {
    pub fn new(host: Host, stream_supervisor: Addr<TimelineStreamSupervisorActor>) -> Self {
        Self {
            stream_supervisor,
            host,
            stream: None,
        }
    }
}

#[async_trait]
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for TimelineStreamWebsocket {
    fn started(&mut self, ctx: &mut Self::Context) {
        let host = self.host.clone();
        let supervisor = self.stream_supervisor.clone();
        let self_addr = ctx.address();
        actix::spawn(async move {
            // TODO: Do not unwrap
            let stream = supervisor
                .send(RequestTimelineStreamer::new(host))
                .await
                .unwrap()
                .unwrap();

            stream.do_send(RequestTimelineStream::new(self_addr.recipient()));
        });
    }

    fn handle(&mut self, item: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match item {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(_) => (),
            Err(_) => todo!(),
        }
    }
}

impl Handler<TimelineMessage> for TimelineStreamWebsocket {
    type Result = ();

    fn handle(&mut self, msg: TimelineMessage, ctx: &mut Self::Context) -> Self::Result {
        ctx.text(serde_json::to_string(msg.get()).unwrap());
    }
}
