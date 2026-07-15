//! Thin AgentOS-facing wrapper around RivetKit's portable actor context.
//!
//! RivetKit owns the ABI bridge, completion channels, context refcounts, and
//! pushed-event queue. AgentOS keeps only the small string/byte adaptations its
//! persistence and action modules use.

#![allow(dead_code)]

use anyhow::anyhow;
use rivet_actor_plugin_abi::{DylibBackend, Event, PortableActorCtx, ReplyToken};

#[derive(Clone)]
pub(crate) struct HostCtx {
    inner: PortableActorCtx,
}

impl HostCtx {
    pub(crate) fn from_backend(backend: DylibBackend) -> Self {
        Self {
            inner: PortableActorCtx::new_dylib(backend),
        }
    }

    pub(crate) async fn db_exec(&self, sql: Vec<u8>) -> Result<Vec<u8>, String> {
        let sql = std::str::from_utf8(&sql).map_err(|error| format!("sql utf8: {error}"))?;
        self.inner
            .db_exec(sql)
            .await
            .map_err(|error| format!("{error:#}"))
    }

    pub(crate) async fn db_query(&self, sql: Vec<u8>, params: Vec<u8>) -> Result<Vec<u8>, String> {
        let sql = std::str::from_utf8(&sql).map_err(|error| format!("sql utf8: {error}"))?;
        self.inner
            .db_query(sql, (!params.is_empty()).then_some(params))
            .await
            .map_err(|error| format!("{error:#}"))
    }

    pub(crate) async fn db_run(&self, sql: Vec<u8>, params: Vec<u8>) -> Result<Vec<u8>, String> {
        let sql = std::str::from_utf8(&sql).map_err(|error| format!("sql utf8: {error}"))?;
        self.inner
            .db_run(sql, (!params.is_empty()).then_some(params))
            .await
            .map(|()| Vec::new())
            .map_err(|error| format!("{error:#}"))
    }

    pub(crate) async fn next_event(&self) -> Option<Event> {
        match self.inner.next_event().await {
            Ok(event) => event,
            Err(error) => {
                self.log_warn(&format!("native actor event stream failed: {error:#}"));
                None
            }
        }
    }

    pub(crate) fn sql_is_enabled(&self) -> bool {
        self.inner.sql_is_enabled()
    }

    pub(crate) fn startup_ready(&self, ok: bool, message: &str) {
        let result = if ok {
            Ok(())
        } else {
            Err(anyhow!(message.to_owned()))
        };
        if let Err(error) = self.inner.startup_ready(result) {
            self.log_warn(&format!("failed to signal actor startup: {error:#}"));
        }
    }

    pub(crate) fn reply_ok(&self, token: u64, payload: Vec<u8>) {
        if let Err(error) = self.inner.reply_ok(ReplyToken(token), payload) {
            self.log_warn(&format!("failed to complete actor reply: {error:#}"));
        }
    }

    pub(crate) fn reply_err(&self, token: u64, message: &str) {
        if let Err(error) = self.inner.reply_err(ReplyToken(token), message) {
            self.log_warn(&format!("failed to complete actor error reply: {error:#}"));
        }
    }

    pub(crate) fn broadcast(&self, name: Vec<u8>, payload: Vec<u8>) {
        let name = match String::from_utf8(name) {
            Ok(name) => name,
            Err(error) => {
                self.log_warn(&format!(
                    "failed to broadcast non-UTF-8 event name: {error}"
                ));
                return;
            }
        };
        if let Err(error) = self.inner.broadcast(name, payload) {
            self.log_warn(&format!("failed to broadcast actor event: {error:#}"));
        }
    }

    pub(crate) fn log_warn(&self, message: &str) {
        self.inner.log(3, message);
    }
}
