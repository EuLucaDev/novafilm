//! Colaborar (Nova Live): convidar pessoas para editar o mesmo projeto ao vivo, entrar por um convite e
//! ver quem está na sessão. A rede fica em `nova_live`; o motor sabe o que replicar (`live`).

use egui::{Context, RichText};
use nova_live::{Change, Invite, Live, Notice, Peer, Role};
use novafilm_engine::live::LiveChange;

use crate::NovafilmApp;

const APP: &str = "novafilm";

#[derive(Default)]
pub struct LiveUi {
    pub live: Option<Live>,
    pub open: bool,
    document_ready: bool,
    pub invite_text: String,
    pub peers: Vec<Peer>,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// Convite recebido pela linha de comando (`--join <link>`), aberto no primeiro quadro.
    pub pending_join: Option<String>,
}

impl LiveUi {
    pub fn active(&self) -> bool {
        self.live.is_some()
    }
}

fn my_name() -> String {
    std::env::var("NOVA_FOODS_NAME").or_else(|_| std::env::var("USER")).or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "Nova Foods".into())
}

fn toast(app: &mut NovafilmApp, msg: impl Into<String>) {
    let msg: String = msg.into();
    app.live.notice = Some(msg);
}

/// Começa a compartilhar o mesmo projeto ativo.
pub fn host(app: &mut NovafilmApp) {
    if app.session.live_start().is_err() {
        app.live.error = Some("Abra um projeto para colaborar.".into());
        return;
    }
    match Live::host(APP, nova_live::DEFAULT_SERVER, &my_name()) {
        Ok(live) => {
            app.live.live = Some(live);
            app.live.error = None;
        }
        Err(e) => {
            app.session.live_stop();
            app.live.error = Some(format!("Não foi possível iniciar a colaboração: {e}"));
        }
    }
}

/// Entra num documento compartilhado pelo convite (link do app ou da página de convite).
pub fn join(app: &mut NovafilmApp, text: &str) {
    let invite = match Invite::parse(text) {
        Ok(i) => i,
        Err(e) => {
            app.live.error = Some(e.to_string());
            return;
        }
    };
    if invite.app != APP {
        app.live.error = Some(format!("Este convite é de outro app ({}). Abra-o no app certo pelo Nova Foods Launcher.", invite.app));
        return;
    }
    if app.session.is_dirty() {
        app.live.error = Some("Salve seu trabalho antes de abrir o convite.".into());
        return;
    }
    if let Err(e) = app.session.execute("file.newProject", serde_json::json!({"name": "Projeto compartilhado"})) {
        app.live.error = Some(e.to_string());
        return;
    }
    if app.session.live_start().is_err() {
        app.live.error = Some("Não foi possível preparar o documento compartilhado.".into());
        return;
    }
    match Live::join(&invite, APP, &my_name()) {
        Ok(live) => {
            app.live.live = Some(live);
            app.live.error = None;
            app.live.open = true;
        }
        Err(e) => {
            app.session.live_stop();
            app.live.error = Some(format!("Não foi possível entrar: {e}"));
        }
    }
}

pub fn stop(app: &mut NovafilmApp) {
    app.live.live = None;
    app.live.peers.clear();
    app.live.document_ready = false;
    app.session.live_stop();
}

/// O motor visto pela sessão.
struct Doc<'a>(&'a mut novafilm_engine::Session);

fn e(err: impl std::fmt::Display) -> nova_live::Error {
    nova_live::Error(err.to_string())
}

impl nova_live::Adapter for Doc<'_> {
    fn take_changes(&mut self) -> Vec<Change> {
        self.0
            .live_take()
            .into_iter()
            .map(|c| match c {
                LiveChange::Op { cmd, params, ctx } => Change::Op { cmd, params, ctx },
                LiveChange::Replace => Change::Replace,
            })
            .collect()
    }
    fn snapshot(&mut self) -> nova_live::Result<Vec<u8>> {
        self.0.live_snapshot().map_err(e)
    }
    fn load(&mut self, doc: &[u8]) -> nova_live::Result<()> {
        self.0.live_load(doc).map_err(e)
    }
    fn apply(&mut self, cmd: &str, params: &serde_json::Value, ctx: &serde_json::Value) -> nova_live::Result<()> {
        self.0.live_apply(cmd, params, ctx).map_err(e)
    }
    fn digest(&mut self) -> nova_live::Result<u64> {
        self.0.live_digest().map_err(e)
    }
    fn busy(&self) -> bool {
        self.0.live_busy()
    }
}

/// A cada quadro: envia o que mudou aqui e aplica o que veio dos colegas.
pub fn pump(app: &mut NovafilmApp, ctx: &Context) {
    if let Some(text) = app.live.pending_join.take() {
        app.live.open = true;
        join(app, &text);
    }
    if app.live.live.is_some() && app.session.live_active() { app.live.document_ready = true; }
    if app.live.document_ready && !app.session.live_active() {
        stop(app);
        app.live.error = Some("A colaboração terminou porque o arquivo compartilhado foi fechado ou substituído.".into());
        return;
    }
    let Some(live) = app.live.live.as_mut() else { return };
    let notices = live.pump(&mut Doc(&mut app.session));
    app.live.peers = live.peers().to_vec();
    for n in notices {
        match n {
            Notice::Joined(name) => toast(app, format!("{name} entrou")),
            Notice::Left(name) => toast(app, format!("{name} saiu")),
            Notice::Problem(p) => toast(app, format!("Colaboração: {p}")),
            Notice::Ended(reason) => {
                toast(app, reason);
                stop(app);
                return;
            }
        }
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(50));
}

/// O botão da barra de título: "Colaborar" ou "Ao vivo · N".
pub fn button(app: &mut NovafilmApp, ui: &mut egui::Ui) {
    let label = if app.live.active() { format!("Ao vivo · {}", app.live.peers.len()) } else { "Colaborar".to_string() };
    if ui.add(egui::Button::new(label)).on_hover_text("Editar ao vivo com outras pessoas").clicked() {
        app.live.open = true;
    }
}

/// A janela Colaborar.
pub fn window(app: &mut NovafilmApp, ctx: &Context) {
    if !app.live.open {
        return;
    }
    let mut open = true;
    let mut action: Option<&'static str> = None;
    egui::Window::new("Colaborar").open(&mut open).collapsible(false).resizable(false).default_width(420.0).show(ctx, |ui| {
        if let Some(live) = app.live.live.as_ref() {
            let host = live.role() == Role::Host;
            ui.label(RichText::new(if host { "Você está compartilhando este projeto." } else { "Você está em um projeto compartilhado." }).strong());
            ui.add_space(6.0);
            if host {
                let link = live.invite().to_share_link();
                ui.label("Envie este convite para quem vai editar junto. Ele vale só para este projeto; quem não tiver o Nova Film instala pelo Nova Foods Launcher.");
                ui.add_space(4.0);
                let mut shown = link.clone();
                ui.add(egui::TextEdit::singleline(&mut shown).desired_width(f32::INFINITY));
                if live.peers().is_empty() { ui.spinner(); ui.label("Conectando à colaboração…"); }
                if ui.add_enabled(!live.peers().is_empty(), egui::Button::new("Copiar convite")).clicked() {
                    ui.ctx().copy_text(link);
                    action = Some("copied");
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Participantes agora ({})", app.live.peers.len())).strong());
            for p in &app.live.peers {
                ui.label(format!("{}{}", p.name, if p.host { " · anfitrião" } else { "" }));
            }
            ui.add_space(8.0);
            if ui.button(if host { "Encerrar colaboração" } else { "Sair" }).clicked() {
                action = Some("stop");
            }
        } else {
            ui.label("Edite este projeto ao vivo com outras pessoas.");
            ui.add_space(4.0);
            if ui.button("Convidar pessoas").clicked() {
                action = Some("host");
            }
            ui.separator();
            ui.label("Recebeu um convite? Cole aqui:");
            ui.add(egui::TextEdit::singleline(&mut app.live.invite_text).hint_text("https://workflowind.agcunae.com/apps/join#invite=…").desired_width(f32::INFINITY));
            if ui.button("Entrar").clicked() {
                action = Some("join");
            }
        }
        if let Some(note) = &app.live.notice { ui.label(note); }
        if let Some(e) = &app.live.error {
            ui.add_space(6.0);
            ui.colored_label(egui::Color32::from_rgb(0xd6, 0x28, 0x39), e);
        }
    });
    if !open {
        app.live.open = false;
    }
    match action {
        Some("host") => host(app),
        Some("join") => {
            let text = app.live.invite_text.clone();
            join(app, &text);
        }
        Some("stop") => stop(app),
        Some("copied") => toast(app, "Convite copiado"),
        _ => {}
    }
}

/// A faixa usa o tema do app e mantém a colaboração acessível em todas as áreas.
pub fn bar(app: &mut NovafilmApp, ui: &mut egui::Ui) {
    egui::Panel::top("nova-live-bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Nova Foods").strong());
            ui.separator();
            ui.label(if app.live.active() { "Arquivo compartilhado" } else { "Colabore neste arquivo" });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| button(app, ui));
        });
    });
}

/// Keep private invitation tokens out of file-open paths and their diagnostic logs.
pub fn is_invite_arg(arg: &str) -> bool {
    arg == "--join" || arg.starts_with("novalive:") || arg.starts_with("https://workflowind.agcunae.com/apps/join#invite=")
}

pub fn launch_invite() -> Option<String> {
    std::env::args_os().skip(1).filter_map(|arg| arg.into_string().ok())
        .find(|arg| arg != "--join" && is_invite_arg(arg))
}
