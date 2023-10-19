use std::{
    collections::HashMap,
    time::{self, Duration},
};

use actix::{
    Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, Context, Handler, Message, Recipient,
    ResponseActFuture, StreamHandler, WrapFuture,
};
use anyhow::Result;
use futures_util::Stream;
use log::info;

use crate::{timeline_stream::stream::get_timeline_stream, types::Host};
use crate::types::json::Post;

pub static HEARTBEAT_DURATION: Duration = Duration::from_secs(1);
static HEARTBEAT_DURATION_TRELANCE: Duration = Duration::from_secs(2);

// Actors
pub struct TimelineStreamSupervisorActor {
    streamers: HashMap<Host, Addr<TimelineStreamerActor>>,
}

impl Actor for TimelineStreamSupervisorActor {
    type Context = Context<Self>;
}

impl Handler<TimelineStreamerStopMessage> for TimelineStreamSupervisorActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: TimelineStreamerStopMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let mut host_to_remove = None;

        // TODO: 最後にStreamerを誰かに紹介したら削除させない

        for (host, addr) in &self.streamers {
            if addr == &msg.addr {
                host_to_remove = Some(host.clone());
            }
        }

        if let Some(host) = host_to_remove {
            self.streamers.remove(&host);
        }
    }
}

impl Handler<RequestTimelineStreamer> for TimelineStreamSupervisorActor {
    type Result = ResponseActFuture<Self, Result<Addr<TimelineStreamerActor>>>;

    fn handle(&mut self, msg: RequestTimelineStreamer, ctx: &mut Self::Context) -> Self::Result {
        if let Some(streamer) = self.streamers.get(&msg.host) {
            let streamer = streamer.clone();
            return Box::pin(async move { Ok(streamer) }.into_actor(self));
        } else {
            let self_address = ctx.address();

            let host = msg.host.clone();
            let host2 = msg.host.clone();
            Box::pin(
                async {
                    let new_stream = get_timeline_stream(host).await.unwrap().get_stream();

                    TimelineStreamerActor::new(new_stream, self_address)
                }
                .into_actor(self)
                .map(move |streamer, actor, _| {
                    actor.streamers.insert(host2, streamer.clone());
                    Ok(streamer)
                }),
            )
        }
    }
}

impl TimelineStreamSupervisorActor {
    pub fn new() -> Addr<Self> {
        TimelineStreamSupervisorActor::create(|_| Self {
            streamers: HashMap::new(),
        })
    }
}

pub struct TimelineStreamerActor {
    supervisor: Addr<TimelineStreamSupervisorActor>,
    last_heartbeat: HashMap<Recipient<TimelineMessage>, time::Instant>,
}

impl TimelineStreamerActor {
    pub fn new(
        timeline_stream: impl Stream<Item = Post> + 'static,
        supervisor: Addr<TimelineStreamSupervisorActor>,
    ) -> Addr<Self> {
        Self::create(|ctx| {
            ctx.add_stream(timeline_stream);

            TimelineStreamerActor {
                last_heartbeat: HashMap::new(),
                supervisor,
            }
        })
    }
}

impl Actor for TimelineStreamerActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("Started TimelineStreamerActor");
        ctx.run_interval(HEARTBEAT_DURATION_TRELANCE, |actor, ctx| {
            let mut remove_list = Vec::new();

            for (receiver, last_heartbeat) in &actor.last_heartbeat {
                if last_heartbeat.elapsed() > HEARTBEAT_DURATION_TRELANCE {
                    remove_list.push(receiver.clone());
                }
            }

            for to_remove in remove_list {
                info!("An recipient is disconnected from TimelineStreamerActor");
                actor.last_heartbeat.remove(&to_remove);
            }

            if actor.last_heartbeat.len() == 0 {
                ctx.stop();
            }
        });
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> actix::Running {
        info!("Stopping TimelineStreamerActor");
        // TODO: Check supervisor state before stop
        self.supervisor.do_send(TimelineStreamerStopMessage {
            addr: ctx.address(),
        });
        actix::Running::Stop
    }
}

impl StreamHandler<Post> for TimelineStreamerActor {
    fn handle(&mut self, item: Post, _ctx: &mut Self::Context) {
        for (recipient, _last_heartbeat) in &self.last_heartbeat {
            recipient.do_send(TimelineMessage { post: item.clone() });
        }
    }
}

impl Handler<HeartbeatMessage> for TimelineStreamerActor {
    type Result = ();

    fn handle(&mut self, msg: HeartbeatMessage, _: &mut Self::Context) -> Self::Result {
        self.last_heartbeat.insert(msg.addr, time::Instant::now());
    }
}

impl Handler<RequestTimelineStream> for TimelineStreamerActor {
    type Result = ();

    fn handle(&mut self, msg: RequestTimelineStream, _ctx: &mut Self::Context) -> Self::Result {
        self.last_heartbeat.insert(msg.addr, time::Instant::now());
    }
}

// Messages
pub struct HeartbeatMessage {
    addr: Recipient<TimelineMessage>,
}

impl Message for HeartbeatMessage {
    type Result = ();
}

impl HeartbeatMessage {
    pub fn new(addr: Recipient<TimelineMessage>) -> Self {
        Self {
            addr
        }
    }
}

pub struct TimelineMessage {
    post: Post,
}

impl TimelineMessage {
    pub fn get(&self) -> &Post {
        &self.post
    }
}

impl Message for TimelineMessage {
    type Result = ();
}

struct TimelineStreamerStopMessage {
    addr: Addr<TimelineStreamerActor>,
}

impl Message for TimelineStreamerStopMessage {
    type Result = ();
}

pub struct RequestTimelineStreamer {
    host: Host,
}

impl RequestTimelineStreamer {
    pub fn new(host: Host) -> Self {
        Self { host }
    }
}

impl Message for RequestTimelineStreamer {
    type Result = Result<Addr<TimelineStreamerActor>>;
}

pub struct RequestTimelineStream {
    addr: Recipient<TimelineMessage>
}

impl RequestTimelineStream {
    pub fn new(addr: Recipient<TimelineMessage>) -> Self {
        Self {
            addr
        }
    }
}

impl Message for RequestTimelineStream {
    type Result = ();
}