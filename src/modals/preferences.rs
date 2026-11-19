use adw::prelude::*;
use gettextrs::gettext;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

use crate::config::APP_ID;

pub struct PreferencesDialog {
    settings: gtk::gio::Settings,
}

#[derive(Debug)]
pub enum PreferencesOutput {
    GridSizeChanged,
    HelpersChanged,
}

#[derive(Debug)]
pub enum SettingValue {
    Bool(bool),
    Int(i32),
}

#[derive(Debug)]
pub enum PreferencesMsg {
    SettingChanged(&'static str, SettingValue),
}

impl SimpleComponent for PreferencesDialog {
    type Root = adw::PreferencesDialog;
    type Widgets = adw::PreferencesDialog;
    type Init = ();
    type Input = PreferencesMsg;
    type Output = PreferencesOutput;

    fn init_root() -> Self::Root {
        adw::PreferencesDialog::builder().build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let settings = gtk::gio::Settings::new(*APP_ID);

        let grid_size = adw::SpinRow::with_range(3.0, 10.0, 1.0);
        grid_size.set_title(&gettext("Grid size"));
        grid_size.set_value(settings.int("grid-size").clamp(3, 10) as f64);

        let grid_sender = sender.clone();
        grid_size.connect_value_notify(move |row| {
            grid_sender.input(PreferencesMsg::SettingChanged(
                "grid-size",
                SettingValue::Int(row.value().round() as i32),
            ));
        });

        let game_group = adw::PreferencesGroup::builder()
            .title(gettext("Game"))
            .build();
        game_group.add(&grid_size);

        let errors = adw::SwitchRow::builder()
            .title(gettext("Show errors"))
            .active(settings.boolean("show-errors"))
            .build();

        let errors_sender = sender.clone();
        errors.connect_active_notify(move |row| {
            errors_sender.input(PreferencesMsg::SettingChanged(
                "show-errors",
                SettingValue::Bool(row.is_active()),
            ));
        });

        let background = adw::SwitchRow::builder()
            .title(gettext("Show image as background"))
            .active(settings.boolean("show-background"))
            .build();

        let background_sender = sender.clone();
        background.connect_active_notify(move |row| {
            background_sender.input(PreferencesMsg::SettingChanged(
                "show-background",
                SettingValue::Bool(row.is_active()),
            ));
        });

        let helpers_group = adw::PreferencesGroup::builder()
            .title(gettext("Helpers"))
            .build();
        helpers_group.add(&errors);
        helpers_group.add(&background);

        let page = adw::PreferencesPage::builder()
            .title(gettext("Preferences"))
            .icon_name("preferences-system-symbolic")
            .build();
        page.add(&game_group);
        page.add(&helpers_group);

        root.add(&page);
        root.present(Some(&relm4::main_application().windows()[0]));

        let initial_grid_size = settings.int("grid-size");
        let initial_errors = settings.boolean("show-errors");
        let initial_background = settings.boolean("show-background");

        let settings_clone = settings.clone();
        root.connect_closed(move |_| {
            if settings_clone.int("grid-size") != initial_grid_size {
                sender.output(PreferencesOutput::GridSizeChanged).unwrap();
            }
            if settings_clone.boolean("show-errors") != initial_errors
                || settings_clone.boolean("show-background") != initial_background
            {
                sender.output(PreferencesOutput::HelpersChanged).unwrap();
            }
        });

        let widgets = root.clone();
        let model = PreferencesDialog { settings };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            PreferencesMsg::SettingChanged(key, SettingValue::Int(value)) => {
                self.settings.set_int(key, value).unwrap();
            }
            PreferencesMsg::SettingChanged(key, SettingValue::Bool(value)) => {
                self.settings.set_boolean(key, value).unwrap();
            }
        }
    }
}
