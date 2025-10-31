use anyhow::{anyhow, Context, Result};
use clap::Parser;
use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1::{Event as CosmicHandleEvent, ZcosmicToplevelHandleV1},
    zcosmic_toplevel_info_v1::{
        Event as CosmicInfoEvent, ZcosmicToplevelInfoV1, EVT_TOPLEVEL_OPCODE,
    },
};
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1;
use wayland_client::{
    event_created_child,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{Event as ForeignToplevelEvent, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{
        Event as ForeignListEvent, ExtForeignToplevelListV1,
        EVT_TOPLEVEL_OPCODE as FOREIGN_TOPLEVEL_OPCODE,
    },
};

type CosmicToplevelInfo = ZcosmicToplevelInfoV1;
type CosmicToplevelHandle = ZcosmicToplevelHandleV1;
type CosmicToplevelManager = ZcosmicToplevelManagerV1;
type ForeignToplevelList = ExtForeignToplevelListV1;
type ForeignToplevelHandle = ExtForeignToplevelHandleV1;

/// Launch or focus an application by app-id / desktop-id (ex: org.mozilla.firefox or firefox)
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// App ID (Wayland app_id or desktop file ID)
    app_id: String,
    /// Command to launch if not running (default: gtk-launch <app_id>)
    #[arg(long)]
    launch_cmd: Option<String>,
    /// Increase logging verbosity (-v, -vv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(Clone)]
struct TrackedToplevel {
    foreign: Option<ForeignToplevelHandle>,
    cosmic: Option<CosmicToplevelHandle>,
    app_id: Option<String>,
}

impl TrackedToplevel {
    fn matches_foreign(&self, handle: &ForeignToplevelHandle) -> bool {
        self.foreign
            .as_ref()
            .map(|stored| stored.id() == handle.id())
            .unwrap_or(false)
    }

    fn matches_cosmic(&self, handle: &CosmicToplevelHandle) -> bool {
        self.cosmic
            .as_ref()
            .map(|stored| stored.id() == handle.id())
            .unwrap_or(false)
    }
}

struct State {
    target_lc: String,
    seat: Option<wl_seat::WlSeat>,
    info: Option<CosmicToplevelInfo>,
    mgr: Option<CosmicToplevelManager>,
    foreign_list: Option<ForeignToplevelList>,
    toplevels: Vec<TrackedToplevel>,
    match_handle: Option<CosmicToplevelHandle>,
}

impl State {
    fn new(target: String) -> Self {
        Self {
            target_lc: target.to_lowercase(),
            seat: None,
            info: None,
            mgr: None,
            foreign_list: None,
            toplevels: Vec::new(),
            match_handle: None,
        }
    }

    fn app_matches(&self, app_id: &str) -> bool {
        let candidate = app_id.to_lowercase();
        if candidate == self.target_lc {
            return true;
        }
        candidate.ends_with(&format!(".{}", self.target_lc))
            || self.target_lc.ends_with(&format!(".{}", candidate))
    }

    fn update_match_for_index(&mut self, idx: usize) {
        if let (Some(ref cosmic), Some(ref app_id)) =
            (&self.toplevels[idx].cosmic, &self.toplevels[idx].app_id)
        {
            if self.app_matches(app_id) {
                log::info!(
                    "Matched target app '{}' via cosmic handle {}",
                    app_id,
                    cosmic.id()
                );
                self.match_handle = Some(cosmic.clone());
            }
        }
    }

    fn remove_by_foreign(&mut self, handle: &ForeignToplevelHandle) {
        let remove_id = handle.id();
        log::debug!("Foreign toplevel {} closed", remove_id);
        self.toplevels.retain(|tracked| {
            tracked
                .foreign
                .as_ref()
                .map(|f| f.id() != remove_id)
                .unwrap_or(true)
        });
        self.drop_match_if_stale();
    }

    fn remove_by_cosmic(&mut self, handle: &CosmicToplevelHandle) {
        let remove_id = handle.id();
        log::debug!("Cosmic toplevel {} closed", remove_id);
        self.toplevels.retain(|tracked| {
            tracked
                .cosmic
                .as_ref()
                .map(|c| c.id() != remove_id)
                .unwrap_or(true)
        });
        self.drop_match_if_stale();
    }

    fn drop_match_if_stale(&mut self) {
        if let Some(ref matched) = self.match_handle {
            let keep = self.toplevels.iter().any(|tracked| {
                tracked
                    .cosmic
                    .as_ref()
                    .map(|c| c.id() == matched.id())
                    .unwrap_or(false)
            });
            if !keep {
                self.match_handle = None;
            }
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if state.seat.is_none() {
            state.seat = Some(seat.clone());
        }
    }
}

impl Dispatch<ForeignToplevelList, ()> for State {
    fn event(
        state: &mut Self,
        _list: &ForeignToplevelList,
        event: ForeignListEvent,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            ForeignListEvent::Toplevel { toplevel } => {
                let cosmic_handle = state.info.as_ref().and_then(|info| {
                    if info.version() >= 2 {
                        Some(info.get_cosmic_toplevel(&toplevel, qh, ()))
                    } else {
                        None
                    }
                });
                let cosmic_id = cosmic_handle.as_ref().map(|handle| handle.id());
                log::debug!(
                    "Foreign toplevel {} announced (cosmic handle {:?})",
                    toplevel.id(),
                    cosmic_id
                );

                state.toplevels.push(TrackedToplevel {
                    foreign: Some(toplevel.clone()),
                    cosmic: cosmic_handle,
                    app_id: None,
                });
            }
            ForeignListEvent::Finished => {}
            _ => {}
        }
    }

    event_created_child!(
        State,
        ForeignToplevelList,
        [
            FOREIGN_TOPLEVEL_OPCODE => (ForeignToplevelHandle, ())
        ]
    );
}

impl Dispatch<ForeignToplevelHandle, ()> for State {
    fn event(
        state: &mut Self,
        toplevel: &ForeignToplevelHandle,
        event: ForeignToplevelEvent,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ForeignToplevelEvent::AppId { app_id } => {
                if let Some(idx) = state
                    .toplevels
                    .iter()
                    .enumerate()
                    .find_map(|(idx, tracked)| tracked.matches_foreign(toplevel).then_some(idx))
                {
                    log::debug!(
                        "Foreign toplevel {} reports app_id '{}'",
                        toplevel.id(),
                        app_id
                    );
                    state.toplevels[idx].app_id = Some(app_id.clone());
                    state.update_match_for_index(idx);
                }
            }
            ForeignToplevelEvent::Closed => {
                state.remove_by_foreign(toplevel);
            }
            _ => {}
        }
    }
}

impl Dispatch<CosmicToplevelHandle, ()> for State {
    fn event(
        state: &mut Self,
        handle: &CosmicToplevelHandle,
        event: CosmicHandleEvent,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            CosmicHandleEvent::AppId { app_id } => {
                if let Some(idx) = state
                    .toplevels
                    .iter()
                    .enumerate()
                    .find_map(|(idx, tracked)| tracked.matches_cosmic(handle).then_some(idx))
                {
                    log::debug!("Cosmic handle {} reports app_id '{}'", handle.id(), app_id);
                    state.toplevels[idx].app_id = Some(app_id.clone());
                    state.update_match_for_index(idx);
                } else {
                    let matches = state.app_matches(&app_id);
                    state.toplevels.push(TrackedToplevel {
                        foreign: None,
                        cosmic: Some(handle.clone()),
                        app_id: Some(app_id.clone()),
                    });
                    if matches {
                        log::info!(
                            "Matched target app '{}' via standalone cosmic handle {}",
                            app_id,
                            handle.id()
                        );
                        state.match_handle = Some(handle.clone());
                    }
                }
            }
            CosmicHandleEvent::Closed => {
                state.remove_by_cosmic(handle);
            }
            _ => {}
        }
    }
}

impl Dispatch<CosmicToplevelInfo, ()> for State {
    fn event(
        _state: &mut Self,
        _info: &CosmicToplevelInfo,
        _event: CosmicInfoEvent,
        _data: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(
        State,
        CosmicToplevelInfo,
        [
            EVT_TOPLEVEL_OPCODE => (CosmicToplevelHandle, ())
        ]
    );
}

impl Dispatch<CosmicToplevelManager, ()> for State {
    fn event(
        _: &mut Self,
        _: &CosmicToplevelManager,
        _: <CosmicToplevelManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn init_logger(verbosity: u8) {
    let level = match verbosity {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level));
    builder.format_timestamp_millis();
    let _ = builder.try_init();
}

fn main() -> Result<()> {
    let args = Args::parse();
    init_logger(args.verbose);
    log::debug!("Starting focus helper for {}", args.app_id);
    let launch_cmd = args
        .launch_cmd
        .unwrap_or_else(|| format!("gtk-launch {}", args.app_id));
    log::debug!("Launch fallback command: {}", launch_cmd);

    let conn = Connection::connect_to_env().context("connect to Wayland")?;
    log::debug!("Connected to Wayland display");
    let (globals, mut event_queue) = registry_queue_init::<State>(&conn)?;
    let qh = event_queue.handle();

    let mut state = State::new(args.app_id.clone());

    if let Ok(seat) = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=8, ()) {
        log::debug!("Bound wl_seat v{}", seat.version());
        state.seat = Some(seat);
    } else {
        log::warn!("No wl_seat available; activation requests may be ignored");
    }

    let info = globals
        .bind::<CosmicToplevelInfo, _, _>(&qh, 1..=3, ())
        .context("bind cosmic_toplevel_info")?;
    log::debug!("Bound cosmic_toplevel_info v{}", info.version());
    if info.version() < 2 {
        log::warn!(
            "cosmic_toplevel_info version {} lacks get_cosmic_toplevel; relying on fallback app_id events",
            info.version()
        );
    }
    state.info = Some(info);

    let mgr = globals
        .bind::<CosmicToplevelManager, _, _>(&qh, 1..=4, ())
        .context("bind cosmic_toplevel_manager")?;
    log::debug!("Bound cosmic_toplevel_manager v{}", mgr.version());
    state.mgr = Some(mgr);

    match globals.bind::<ForeignToplevelList, _, _>(&qh, 1..=1, ()) {
        Ok(list) => {
            log::debug!(
                "Bound ext_foreign_toplevel_list_v1 v{} for richer metadata",
                list.version()
            );
            state.foreign_list = Some(list);
        }
        Err(_) => {
            log::warn!(
                "ext_foreign_toplevel_list_v1 unavailable; relying solely on COSMIC handles"
            );
        }
    }

    for _ in 0..5 {
        log::debug!("Pumping Wayland event queue for discovery");
        event_queue
            .roundtrip(&mut state)
            .context("process wayland events")?;
        if state.match_handle.is_some() {
            break;
        }
    }

    let _ = event_queue.dispatch_pending(&mut state);

    if let (Some(handle), Some(seat), Some(mgr)) = (
        state.match_handle.as_ref(),
        state.seat.as_ref(),
        state.mgr.as_ref(),
    ) {
        mgr.activate(handle, seat);
        log::info!(
            "Requested activation for '{}' (handle {})",
            args.app_id,
            handle.id()
        );
        conn.flush().context("flush activation request")?;
        let _ = event_queue.dispatch_pending(&mut state);
        return Ok(());
    }

    log::info!(
        "No running instance matched; launching '{}' via '{}'",
        args.app_id,
        launch_cmd
    );
    let status = std::process::Command::new("sh")
        .arg("-lc")
        .arg(&launch_cmd)
        .status()
        .map_err(|e| anyhow!("failed to launch: {e}"))?;

    if !status.success() {
        return Err(anyhow!("launcher exited with {}", status));
    }
    Ok(())
}
