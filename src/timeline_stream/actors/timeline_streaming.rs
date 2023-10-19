use std::{
    collections::HashMap,
    pin::pin,
    time::{self, Duration},
};

use actix::{
    Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, Context, Handler, Message, Recipient,
    ResponseActFuture, StreamHandler, WrapFuture,
};
use anyhow::{anyhow, Result};
use futures_util::Stream;

use crate::{stream, timeline_stream::stream::get_timeline_stream, types::Host, Timeline};

static HEARTBEAT_DURATION: Duration = Duration::from_secs(1);
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
        ctx: &mut Self::Context,
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
        timeline_stream: impl Stream<Item = Timeline> + 'static,
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
        ctx.run_interval(Duration::from_secs(60), |actor, ctx| {
            let mut remove_list = Vec::new();

            for (receiver, last_heartbeat) in &actor.last_heartbeat {
                if last_heartbeat.elapsed() > HEARTBEAT_DURATION_TRELANCE {
                    remove_list.push(receiver.clone());
                }
            }

            for to_remove in remove_list {
                actor.last_heartbeat.remove(&to_remove);
            }

            if actor.last_heartbeat.len() == 0 {
                ctx.stop();
            }
        });
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> actix::Running {
        // TODO: Check supervisor state before stop
        self.supervisor.do_send(TimelineStreamerStopMessage {
            addr: ctx.address(),
        });
        actix::Running::Stop
    }
}

impl StreamHandler<Timeline> for TimelineStreamerActor {
    fn handle(&mut self, item: Timeline, ctx: &mut Self::Context) {
        for (recipient, last_heartbeat) in &self.last_heartbeat {
            recipient.do_send(TimelineMessage { timeline: item.clone() });
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

    fn handle(&mut self, msg: RequestTimelineStream, ctx: &mut Self::Context) -> Self::Result {
        self.last_heartbeat.insert(msg.addr, time::Instant::now());
    }
}

// Messages
struct HeartbeatMessage {
    addr: Recipient<TimelineMessage>,
}

impl Message for HeartbeatMessage {
    type Result = ();
}

pub struct TimelineMessage {
    timeline: Timeline,
}

impl TimelineMessage {
    pub fn get(&self) -> &Timeline {
        &self.timeline
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