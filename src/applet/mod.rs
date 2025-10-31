mod config;

use std::{borrow::Cow, collections::BTreeSet, time::Duration};

use config::AppletConfig;
use cosmic::{
    app,
    cosmic_config::{Config, CosmicConfigEntry},
    desktop::fde::{self, get_languages_from_env, DesktopEntry},
    iced::{self, Alignment, Subscription},
    iced_widget::Row,
    surface,
    widget::container,
    Action, Element, Task,
};
use rustc_hash::FxHashMap;

use crate::focus;

const APP_ID: &str = "com.system76.CosmicAppFocusApplet";

#[derive(Debug, Clone)]
struct AppButtonModel {
    app_id: String,
    display_name: String,
    icon_name: Option<String>,
}

pub struct FocusApplet {
    core: cosmic::app::Core,
    config: AppletConfig,
    running: Vec<String>,
    items: Vec<AppButtonModel>,
    locales: Vec<String>,
    desktop_entries: Vec<DesktopEntry>,
    desktop_cache: FxHashMap<String, DesktopEntry>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Activate(String),
    AppsUpdated(Vec<String>),
    RefreshTick,
    ConfigUpdated(AppletConfig),
    Surface(surface::Action),
}

impl FocusApplet {
    fn load_config() -> AppletConfig {
        Config::new(APP_ID, AppletConfig::VERSION)
            .ok()
            .and_then(|cfg| AppletConfig::get_entry(&cfg).ok())
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

        for app_id in self.config.pinned.clone() {
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
                    .pinned
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
        };
        applet.update_desktop_entries();
        applet.rebuild_items();
        let task = Task::perform(
            async {
                match focus::list_running_apps() {
                    Ok(apps) => apps,
                    Err(err) => {
                        log::error!("Failed to list running apps: {err}");
                        Vec::new()
                    }
                }
            },
            |apps| Action::App(Message::AppsUpdated(apps)),
        );
        (applet, task)
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
                Task::none()
            }
            Message::RefreshTick => Task::perform(
                async {
                    match focus::list_running_apps() {
                        Ok(apps) => apps,
                        Err(err) => {
                            log::error!("Failed to list running apps: {err}");
                            Vec::new()
                        }
                    }
                },
                |apps| Action::App(Message::AppsUpdated(apps)),
            ),
            Message::ConfigUpdated(config) => {
                self.config = config;
                self.rebuild_items();
                Task::none()
            }
            Message::Surface(action) => {
                cosmic::task::message(Action::Cosmic(cosmic::app::Action::Surface(action)))
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let tick = iced::time::every(Duration::from_secs(4)).map(|_| Message::RefreshTick);
        let config = self.core.watch_config(APP_ID).map(|update| {
            for err in update.errors {
                log::warn!("Config watch error: {err}");
            }
            Message::ConfigUpdated(update.config)
        });
        Subscription::batch(vec![tick, config])
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
