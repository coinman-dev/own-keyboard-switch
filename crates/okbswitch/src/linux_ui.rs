//! Adapters connecting native passive panels to the common UI event loop.
use okbs_core::{
    PhysKey,
    config::{AutoReplace, Theme},
};
use okbs_platform::{
    AutoreplaceInsertion, AutoreplaceLabels, AutoreplaceUi, InputTarget, Result,
    autoreplace_gate::AutoReplaceGate,
};
use okbs_platform_linux::{
    desktop::LinuxDesktop,
    panels::{Panel, Panels},
};
use okbs_ui::autoreplace_list::{
    AutoreplaceListConfig, AutoreplaceListLabels, AutoreplaceListWindow,
};
use std::{rc::Rc, sync::Arc};

pub struct LinuxAutoreplaceUi {
    panels: Rc<Panels>,
    desktop: LinuxDesktop,
    list: AutoreplaceListWindow,
    gate: Arc<AutoReplaceGate>,
    settings: AutoReplace,
    labels: AutoreplaceLabels,
    theme: Theme,
    target: Option<InputTarget>,
}
impl LinuxAutoreplaceUi {
    pub fn new(
        panels: Rc<Panels>,
        desktop: LinuxDesktop,
        list: AutoreplaceListWindow,
        gate: Arc<AutoReplaceGate>,
        settings: AutoReplace,
        labels: AutoreplaceLabels,
        theme: Theme,
    ) -> Self {
        Self {
            panels,
            desktop,
            list,
            gate,
            settings,
            labels,
            theme,
            target: None,
        }
    }
    fn position(&self) -> [f32; 2] {
        self.desktop
            .popup_position(self.target)
            .unwrap_or([24.0, 24.0])
    }
    fn list_config(&self) -> AutoreplaceListConfig {
        AutoreplaceListConfig {
            settings: self.settings.clone(),
            theme: self.theme,
            labels: AutoreplaceListLabels {
                title: self.labels.title.clone(),
                insert: self.labels.insert.clone(),
                close: self.labels.close.clone(),
                empty: self.labels.empty.clone(),
                disabled: self.labels.disabled.clone(),
                menu_help: self.labels.menu_help.clone(),
                list_help: self.labels.list_help.clone(),
            },
        }
    }
    fn theme_name(&self) -> String {
        match self.theme {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::System => "system",
        }
        .into()
    }
}
impl AutoreplaceUi for LinuxAutoreplaceUi {
    fn configure(
        &mut self,
        settings: &AutoReplace,
        labels: AutoreplaceLabels,
        theme: Theme,
    ) -> Result<()> {
        self.settings = settings.clone();
        self.labels = labels;
        self.theme = theme;
        self.list.configure(self.list_config());
        self.panels.hide("hint")?;
        if self.panels.visible("list") {
            self.show_list(true, self.target)?;
            self.show_list(true, self.target)?;
        }
        Ok(())
    }
    fn hint(&mut self, index: Option<usize>) -> Result<()> {
        let Some(item) = index.and_then(|index| self.settings.items.get(index)) else {
            return self.panels.hide("hint");
        };
        self.target = self.desktop.target();
        let point = self.position();
        self.panels.show(Panel {
            id: "hint".into(),
            visible: self.settings.enabled,
            position: [point[0] as i32, point[1] as i32],
            text: format!(
                "{} → {}",
                item.from,
                item.to.chars().take(80).collect::<String>()
            ),
            opacity: 1.0,
            theme: self.theme_name(),
            ..Panel::default()
        })
    }
    fn show_list(&mut self, toggle: bool, target: Option<InputTarget>) -> Result<()> {
        self.target = target.or_else(|| self.desktop.target());
        let point = self.position();
        if toggle {
            if self.panels.visible("list") {
                return self.panels.hide("list");
            }
            let rows = self
                .settings
                .items
                .iter()
                .map(|item| {
                    format!(
                        "{} → {}",
                        item.from,
                        item.to.chars().take(60).collect::<String>()
                    )
                })
                .collect();
            self.panels.show(Panel {
                id: "list".into(),
                visible: true,
                position: [point[0] as i32, point[1] as i32],
                text: if self.settings.enabled {
                    self.labels.list_help.clone()
                } else {
                    self.labels.disabled.clone()
                },
                title: self.labels.title.clone(),
                rows,
                opacity: f64::from(self.settings.list_opacity),
                theme: self.theme_name(),
                ..Panel::default()
            })
        } else {
            self.list.show(self.list_config(), false, point);
            self.desktop.place_owned_window(&self.labels.title, point)
        }
    }
    fn poll(&mut self) -> Option<AutoreplaceInsertion> {
        let native = self
            .panels
            .poll("list")
            .filter(|event| event.action == "insert");
        let index = if let Some(event) = native {
            self.target = self.desktop.target().or(self.target);
            Some(event.index)
        } else {
            self.list.poll()
        }?;
        if !self.settings.enabled {
            return None;
        }
        self.gate.suppress_until_release(PhysKey::Enter);
        let item = self.settings.items.get(index)?.clone();
        Some(AutoreplaceInsertion {
            item,
            target: self.target,
        })
    }
}
