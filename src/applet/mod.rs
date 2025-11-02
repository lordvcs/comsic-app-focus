use cosmic::{
    app,
    cosmic_config::{Config, CosmicConfigEntry},
    desktop::fde::{self, get_languages_from_env, DesktopEntry},
    iced::futures::SinkExt,
    iced::{self, Alignment, Subscription},
    iced_widget::Row,
    surface,
    widget::container,
    Action, Element, Task,
};
mod config;
use config::{AppListConfig, APP_LIST_ID};
use cosmic_settings_config::shortcuts::{
    Action as ShortcutAction, Binding, Config as ShortcutConfig,
};
use iced::stream;
use rustc_hash::FxHashMap;
use std::any::TypeId;
use std::{borrow::Cow, collections::BTreeSet, str::FromStr, sync::mpsc, thread};

use crate::focus;

const APP_ID: &str = "com.system76.CosmicAppFocusApplet";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RunningAppsSubscription;

#[derive(Debug, Clone)]
struct AppButtonModel {
    app_id: String,
    display_name: String,
    icon_name: Option<String>,
}

pub struct FocusApplet {
    core: cosmic::app::Core,
    config: AppListConfig,
    running: Vec<String>,
    items: Vec<AppButtonModel>,
    locales: Vec<String>,
    desktop_entries: Vec<DesktopEntry>,
    desktop_cache: FxHashMap<String, DesktopEntry>,
    shortcut_targets: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Activate(String),
    AppsUpdated(Vec<String>),
    ConfigUpdated(AppListConfig),
    Surface(surface::Action),
}

impl FocusApplet {
    fn load_config() -> AppListConfig {
        Config::new(APP_LIST_ID, AppListConfig::VERSION)
            .ok()
            .and_then(|cfg| AppListConfig::get_entry(&cfg).ok())
            .unwrap_or_default()
    }

    fn update_desktop_entries(&mut self) {
        self.desktop_entries = fde::Iter::new(fde::default_paths())
            .filter_map(|path| DesktopEntry::from_path(path, Some(&self.locales)).ok())
            .collect();
    }

    fn desktop_entry(&mut self, app_id: &str) -> DesktopEntry {
        if let Some(entry) = self.desktop_cache.get(app_id) {
            return entry.clone();
        }

        let unicase_appid = fde::unicase::Ascii::new(app_id);
        if let Some(entry) = fde::find_app_by_id(&self.desktop_entries, unicase_appid) {
            let entry = entry.clone();
            self.desktop_cache
                .entry(app_id.to_string())
                .or_insert_with(|| entry.clone());
            return entry;
        }

        self.update_desktop_entries();
        if let Some(entry) = fde::find_app_by_id(&self.desktop_entries, unicase_appid) {
            let entry = entry.clone();
            self.desktop_cache
                .entry(app_id.to_string())
                .or_insert_with(|| entry.clone());
            return entry;
        }

        let mut entry = DesktopEntry {
            appid: app_id.to_string(),
            groups: Default::default(),
            path: Default::default(),
            ubuntu_gettext_domain: None,
        };
        entry.add_desktop_entry("Name".to_string(), app_id.to_string());
        self.desktop_cache
            .entry(app_id.to_string())
            .or_insert_with(|| entry.clone());
        entry
    }

    fn rebuild_items(&mut self) {
        let mut items = Vec::new();
        let mut seen = BTreeSet::new();

        for app_id in self.config.favorites.clone() {
            if let Some(item) = self.entry_metadata(&app_id) {
                let key = item.app_id.to_lowercase();
                if seen.insert(key) {
                    items.push(item);
                }
            }
        }

        let mut extras: Vec<_> = self
            .running
            .iter()
            .filter(|app| {
                !self
                    .config
                    .favorites
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(app))
            })
            .cloned()
            .collect();
        extras.sort();

        for app_id in extras {
            let key = app_id.to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            if let Some(item) = self.entry_metadata(&app_id) {
                seen.insert(key);
                items.push(item);
            }
        }

        self.items = items;
    }

    fn entry_metadata(&mut self, app_id: &str) -> Option<AppButtonModel> {
        if app_id.is_empty() {
            return None;
        }
        let entry = self.desktop_entry(app_id);
        let name = entry
            .full_name(&self.locales)
            .map(Cow::into_owned)
            .unwrap_or_else(|| entry.appid.clone());
        let icon_name = entry.icon().map(|icon| icon.to_string());
        Some(AppButtonModel {
            app_id: entry.appid.clone(),
            display_name: name,
            icon_name,
        })
    }

    fn make_button<'a>(&'a self, item: &'a AppButtonModel) -> Element<'a, Message> {
        let icon_name = item
            .icon_name
            .as_deref()
            .unwrap_or("application-default-icon");
        let icon_button = self
            .core
            .applet
            .icon_button_from_handle(cosmic::widget::icon::from_name(icon_name).handle())
            .on_press_down(Message::Activate(item.app_id.clone()));

        self.core
            .applet
            .applet_tooltip::<Message>(
                icon_button,
                item.display_name.clone(),
                false,
                Message::Surface,
                None,
            )
            .into()
    }

    fn update_shortcut_bindings(&mut self) {
        let targets: Vec<String> = self
            .config
            .favorites
            .iter()
            .filter(|id| !id.is_empty())
            .take(10)
            .cloned()
            .collect();

        if targets == self.shortcut_targets {
            return;
        }

        if let Err(err) = apply_super_shortcuts(&targets) {
            log::error!("Failed to update Super+number shortcuts: {err}");
        } else {
            self.shortcut_targets = targets;
        }
    }
}

impl cosmic::Application for FocusApplet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn init(core: cosmic::app::Core, _flags: ()) -> (Self, app::Task<Message>) {
        let mut applet = Self {
            core,
            config: Self::load_config(),
            running: Vec::new(),
            items: Vec::new(),
            locales: get_languages_from_env(),
            desktop_entries: Vec::new(),
            desktop_cache: FxHashMap::default(),
            shortcut_targets: Vec::new(),
        };
        applet.update_desktop_entries();
        applet.rebuild_items();
        applet.running = match focus::list_running_apps() {
            Ok(apps) => apps,
            Err(err) => {
                log::error!("Failed to list running apps: {err}");
                Vec::new()
            }
        };
        applet.rebuild_items();
        applet.update_shortcut_bindings();
        (applet, Task::none())
    }

    fn core(&self) -> &cosmic::app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::app::Core {
        &mut self.core
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Activate(app_id) => {
                if let Err(err) = focus::focus_or_launch(&app_id, None) {
                    log::error!("Failed to focus {app_id}: {err}");
                }
                Task::none()
            }
            Message::AppsUpdated(apps) => {
                self.running = apps;
                self.rebuild_items();
                self.update_shortcut_bindings();
                Task::none()
            }
            Message::ConfigUpdated(config) => {
                self.config = config;
                self.rebuild_items();
                self.update_shortcut_bindings();
                Task::none()
            }
            Message::Surface(action) => {
                cosmic::task::message(Action::Cosmic(cosmic::app::Action::Surface(action)))
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let config = self.core.watch_config(APP_LIST_ID).map(|update| {
            for err in update.errors {
                log::warn!("Config watch error: {err}");
            }
            Message::ConfigUpdated(update.config)
        });
        Subscription::batch(vec![running_apps_subscription(), config])
    }

    fn view(&self) -> Element<'_, Message> {
        let mut row = Row::new().spacing(6).align_y(Alignment::Center);

        for item in &self.items {
            row = row.push(self.make_button(item));
        }

        container(row).width(iced::Length::Shrink).into()
    }
}

pub fn run() -> cosmic::iced::Result {
    focus::init_logger(0);
    cosmic::applet::run::<FocusApplet>(())
}

fn running_apps_subscription() -> Subscription<Message> {
    Subscription::run_with_id(
        TypeId::of::<RunningAppsSubscription>(),
        stream::channel(16, |mut output| async move {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                if let Err(err) = focus::watch_running_apps(tx) {
                    log::error!("Wayland watcher exited: {err}");
                }
            });

            while let Ok(apps) = rx.recv() {
                if output.send(Message::AppsUpdated(apps)).await.is_err() {
                    break;
                }
            }
        }),
    )
}

fn apply_super_shortcuts(targets: &[String]) -> anyhow::Result<()> {
    let context = ShortcutConfig::context()?;
    let mut entry = ShortcutConfig::get_entry(&context).unwrap_or_default();

    for idx in 0..10 {
        let key = if idx == 9 {
            "Super+0".to_string()
        } else {
            format!("Super+{}", idx + 1)
        };
        if let Ok(binding) = Binding::from_str(&key) {
            entry.custom.0.remove(&binding);
        }
    }

    for (idx, app_id) in targets.iter().enumerate().take(10) {
        let key = if idx == 9 {
            "Super+0".to_string()
        } else {
            format!("Super+{}", idx + 1)
        };
        let binding = Binding::from_str(&key)
            .map_err(|err| anyhow::anyhow!("invalid binding {}: {}", key, err))?;
        entry.custom.0.insert(
            binding,
            ShortcutAction::Spawn(format!("cosmic-app-focus {}", app_id)),
        );
    }

    entry.write_entry(&context)?;
    Ok(())
}
