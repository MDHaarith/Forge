use forge_server::model::{Model, ModelFuture, ModelReply, ModelRequest};
use std::{collections::VecDeque, sync::Mutex};

/// Replies from a fixed script and records every request it receives.
pub struct ScriptedModel {
    pub requests: Mutex<Vec<ModelRequest>>,
    pub replies: Mutex<VecDeque<ModelReply>>,
}
impl ScriptedModel {
    pub fn new(replies: Vec<ModelReply>) -> Self {
        Self {
            requests: Mutex::new(vec![]),
            replies: Mutex::new(replies.into()),
        }
    }
}
impl Model for ScriptedModel {
    fn name(&self) -> String {
        "scripted".into()
    }
    fn respond<'a>(&'a self, request: &'a ModelRequest) -> ModelFuture<'a> {
        self.requests.lock().unwrap().push(request.clone());
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| "script ran out".into());
        Box::pin(async move { reply })
    }
}
