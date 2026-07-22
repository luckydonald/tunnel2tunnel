//! Fake interactive shell tarpit: an attacker's auth attempt is accepted,
//! but never resolves to a real `Entity` — the resulting channel just serves
//! a bogus prompt, echoes back whatever is typed after an artificial delay,
//! and otherwise hangs forever until the client gives up. Modeled on the
//! existing real welcome-channel keepalive loop in `lib.rs`.

use std::time::Duration;

use russh::server::{Handle, Msg};
use russh::{Channel, ChannelMsg};

const ECHO_DELAY: Duration = Duration::from_millis(800);

const PROMPT: &str = "$ ";

/// Serves a bogus shell prompt on `channel`, echoing input with a delay and
/// never actually executing anything. Consumes the channel and hangs until
/// the client disconnects.
pub async fn serve(handle: Handle, mut channel: Channel<Msg>) {
    let ch_id = channel.id();
    let _ = handle.data(ch_id, format!("\r\n{PROMPT}").into_bytes()).await;

    loop {
        match channel.wait().await {
            None | Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
            Some(ChannelMsg::Data { data }) => {
                tokio::time::sleep(ECHO_DELAY).await;
                let mut reply = data.to_vec();
                reply.extend_from_slice(b"\r\nbash: command not found\r\n");
                reply.extend_from_slice(PROMPT.as_bytes());
                if handle.data(ch_id, reply).await.is_err() {
                    break;
                }
            }
            _ => {}
        }
    }
}

