//! Colaboração em tempo real (Nova Live) no motor do Nova Film (um projeto por sessão).
//!
//! - Comandos do diário viram [`LiveChange::Op`] com o estado de edição de quem editou
//!   (sequência ativa, seleção, cursor), para o colega rodar o mesmo comando no mesmo lugar.
//! - Qualquer outra mudança do projeto (desfazer, refazer, gestos) vira [`LiveChange::Replace`].
//! - O estado de edição de cada pessoa continua dela. A mídia (arquivos de vídeo, áudio, imagem)
//!   fica no computador de cada um: o projeto sincroniza a montagem; mídia que falta aparece offline.

use std::sync::Arc;

use novafilm_project::Project;
use serde_json::{Value, json};

use crate::{EditorState, EngineError, Result, Session};

#[derive(Clone, Debug, PartialEq)]
pub enum LiveChange {
    Op { cmd: String, params: Value, ctx: Value },
    Replace,
}

#[derive(Default)]
pub struct LiveState {
    pub on: bool,
    pub outbox: Vec<LiveChange>,
    pub(crate) applying: bool,
    /// O projeto já informado à sessão (só muda de objeto quando o conteúdo muda).
    pub(crate) seen: Option<Arc<Project>>,
    /// Comandos rodando agora (comandos que chamam comandos): só o de fora é replicado.
    pub(crate) depth: u32,
}

fn err(e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("colaboração: {e}"))
}

/// Impressão digital estável (FNV-1a 64).
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// Only audited document edits may execute on a peer. Other local edits travel as data.
fn remote_command_allowed(cmd: &str) -> bool {
    matches!(cmd, "sequence.addTracks")
}

impl Session {
    pub fn live_start(&mut self) -> Result<()> {
        self.live = LiveState { on: true, seen: Some(self.project.clone()), ..LiveState::default() };
        Ok(())
    }

    pub fn live_stop(&mut self) {
        self.live = LiveState::default();
    }

    pub fn live_active(&self) -> bool {
        self.live.on
    }

    pub fn live_take(&mut self) -> Vec<LiveChange> {
        if self.live.on && !self.live.seen.as_ref().is_some_and(|s| Arc::ptr_eq(s, &self.project)) {
            self.live.seen = Some(self.project.clone());
            self.live.outbox.push(LiveChange::Replace);
        }
        std::mem::take(&mut self.live.outbox)
    }

    pub fn live_busy(&self) -> bool {
        false
    }

    pub fn live_snapshot(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&*self.project).map_err(err)
    }

    pub fn live_digest(&self) -> Result<u64> {
        Ok(fnv64(&self.live_snapshot()?))
    }

    pub fn live_load(&mut self, bytes: &[u8]) -> Result<()> {
        let p: Project = serde_json::from_slice(bytes).map_err(err)?;
        self.project = Arc::new(p);
        self.revision += 1;
        self.live.seen = Some(self.project.clone());

        Ok(())
    }

    /// Roda o comando de um colega com o estado de edição dele; desfazer e estado locais ficam.
    pub fn live_apply(&mut self, cmd: &str, params: &Value, ctx: &Value) -> Result<()> {
        if !remote_command_allowed(cmd) {
            return Err(err("comando não permitido na colaboração"));
        }
        let local = self.state.clone();
        let undo_len = self.history.undo.len();
        if let Some(state) = ctx.get("state").and_then(|s| serde_json::from_value::<EditorState>(s.clone()).ok()) {
            self.state = state;
        }
        self.live.applying = true;
        let r = self.execute(cmd, params.clone());
        self.live.applying = false;
        self.history.undo.truncate(undo_len);
        self.state = local;
        self.live.seen = Some(self.project.clone());

        r.map(|_| ())
    }

    /// Chamado por `execute` depois de cada comando de fora.
    pub(crate) fn live_record(&mut self, id: &str, params: &Value, replicable: bool, ctx: Value) {
        if self.live.applying || !self.live.on || self.live.seen.as_ref().is_some_and(|s| Arc::ptr_eq(s, &self.project)) {
            return;
        }
        if replicable && remote_command_allowed(id) && !matches!(id, "edit.undo" | "edit.redo") {
            self.live.outbox.push(LiveChange::Op { cmd: id.to_string(), params: params.clone(), ctx });
            self.live.seen = Some(self.project.clone());
        }
    }
}

pub(crate) fn context(state: &EditorState) -> Value {
    json!({ "state": state })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn session() -> Session {
        let mut s = Session::default();
        s.execute("file.newSequence", json!({"width": 128, "height": 72, "fps": 24})).unwrap();
        s
    }

    #[test]
    fn remote_commands_cannot_open_export_or_execute_code() {
        let (_, mut b) = pair();
        let before = b.live_digest().unwrap();
        for command in ["file.open", "file.saveAs", "file.export", "session.open", "script.run", "app.quit", "lisp.eval"] {
            assert!(b.live_apply(command, &serde_json::json!({"path": "/tmp/not-authorized"}), &serde_json::json!({})).is_err());
            assert_eq!(before, b.live_digest().unwrap());
        }
    }

    fn pair() -> (Session, Session) {
        let mut a = session();
        a.live_start().unwrap();
        let mut b = session();
        b.live_start().unwrap();
        let snap = a.live_snapshot().unwrap();
        b.live_load(&snap).unwrap();
        (a, b)
    }

    fn sync(a: &mut Session, b: &mut Session) {
        for change in a.live_take() {
            match change {
                LiveChange::Op { cmd, params, ctx } => b.live_apply(&cmd, &params, &ctx).unwrap(),
                LiveChange::Replace => {
                    let snap = a.live_snapshot().unwrap();
                    b.live_load(&snap).unwrap();
                }
            }
        }
        assert!(b.live_take().is_empty(), "o que veio de A não volta para A");
    }

    #[test]
    fn edits_replicate_and_undo_travels_as_a_project() {
        let (mut a, mut b) = pair();
        let b_undo = b.history.undo.len();
        a.execute("sequence.addTracks", json!({"video": 1})).unwrap();
        a.execute("sequence.addTracks", json!({"audio": 1})).unwrap();
        sync(&mut a, &mut b);
        assert_eq!(a.live_digest().unwrap(), b.live_digest().unwrap());
        assert_eq!(b.history.undo.len(), b_undo, "o desfazer de B não ganha os passos de A");

        a.execute("edit.undo", json!({})).unwrap();
        sync(&mut a, &mut b);
        assert_eq!(a.live_digest().unwrap(), b.live_digest().unwrap());

        b.execute("sequence.addTracks", json!({"video": 2})).unwrap();
        sync(&mut b, &mut a);
        assert_eq!(a.live_digest().unwrap(), b.live_digest().unwrap());
    }
}
