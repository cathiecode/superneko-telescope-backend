use anyhow::Result;
use async_stream::stream;
use futures_util::{SinkExt, Stream, StreamExt};
use log::{debug, error, log_enabled, warn};

use crate::misskey::MisskeyDirectStreaming;
use crate::{get_remote_software, http_client, types::Host, Strategy};
use crate::types::json::Post;



pub trait StreamingStrategy {
    fn get_streategy_id(&self) -> &'static str;
    fn get_stream(self: Box<Self>) -> Box<dyn Stream<Item = Post> + Unpin>;
}

pub async fn get_timeline_stream(host: Host) -> Result<Box<dyn StreamingStrategy>> {
    match get_remote_software(&host.base_url()).await? {
        Strategy::MisskeyDirect => {
            debug!("Host {} seems to be running Misskey.", &host.base_url());
            Ok(Box::new(MisskeyDirectStreaming::new(host).await?))
        }
    }
}
