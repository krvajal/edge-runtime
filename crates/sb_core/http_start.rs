use std::cell::RefCell;
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::os::fd::RawFd;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;

use deno_core::error::bad_resource;
use deno_core::error::bad_resource_id;
use deno_core::error::AnyError;
use deno_core::OpState;
use deno_core::ResourceId;
use deno_core::{op2, ToJsBuffer};
use deno_http::http_create_conn_resource;
use deno_net::io::UnixStreamResource;
use futures::pin_mut;
use futures::ready;
use futures::Future;
use log::error;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::net::UnixStream;
use tokio::sync::watch;

use crate::conn_sync::ConnSync;
use crate::conn_sync::ConnWatcher;
use serde::Serialize;

struct UnixStream2(UnixStream, Option<watch::Receiver<ConnSync>>);

impl AsyncRead for UnixStream2 {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut Pin::into_inner(self).0).poll_read(cx, buf)
    }
}

impl AsyncWrite for UnixStream2 {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut Pin::into_inner(self).0).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut Pin::into_inner(self).0).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        true
    }

    #[inline]
    fn poll_flush(
        self: Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        if let Some(ref mut sync) = self.1 {
            let fut = sync.wait_for(|it| *it == ConnSync::Recv);

            pin_mut!(fut);
            ready!(fut.poll(cx).map(|it| {
                if let Err(ex) = it {
                    error!("cannot track outbound connection correctly");
                    return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, ex));
                }

                Ok(())
            }))?
        }

        Pin::new(&mut Pin::into_inner(self).0).poll_shutdown(cx)
    }
}

#[op2]
#[serde]
fn op_http_start(
    state: &mut OpState,
    #[smi] stream_rid: ResourceId,
) -> Result<(ResourceId, ResourceId), AnyError> {
    if let Ok(resource_rc) = state.resource_table.take::<UnixStreamResource>(stream_rid) {
        // This connection might be used somewhere else. If it's the case, we cannot proceed with the
        // process of starting a HTTP server on top of this connection, so we just return a bad
        // resource error. See also: https://github.com/denoland/deno/pull/16242
        let resource = Rc::try_unwrap(resource_rc)
            .map_err(|_| bad_resource("Unix stream is currently in use"))?;

        let (read_half, write_half) = resource.into_inner();
        let unix_stream = read_half.reunite(write_half)?;
        let fd = unix_stream.as_raw_fd();
        let watcher = state
            .borrow_mut::<HashMap<RawFd, watch::Receiver<ConnSync>>>()
            .remove(&fd);

        // set a hardcoded address
        let addr: std::net::SocketAddr = "0.0.0.0:9999".parse().unwrap();
        let conn = http_create_conn_resource(
            state,
            UnixStream2(unix_stream, watcher.clone()),
            addr,
            "http",
        )?;

        let conn_watcher = state.resource_table.add(ConnWatcher(watcher));

        return Ok((conn, conn_watcher));
    }

    Err(bad_resource_id())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpUpgradeResult {
    conn_rid: ResourceId,
    conn_type: &'static str,
    read_buf: ToJsBuffer,
}

#[op2(async)]
#[serde]
async fn op_http_upgrade(
    _state: Rc<RefCell<OpState>>,
    #[smi] _rid: ResourceId,
) -> Result<(), AnyError> {
    Ok(())
}

deno_core::extension!(sb_core_http, ops = [op_http_start, op_http_upgrade]);
