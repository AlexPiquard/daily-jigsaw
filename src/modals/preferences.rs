use adw::prelude::*;
use gettextrs::gettext;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

use crate::config::APP_ID;

pub struct PreferencesDialog {
    settings: gtk::gio::Settings,
}

#[derive(Debug)]
pub enum PreferencesMsg {
    GridSizeChanged(i32),
    HelpersChanged(bool),
}

impl SimpleComponent for PreferencesDialog {
    type Root = adw::PreferencesDialog;
    type Widgets = adw::PreferencesDialog;
    type Init = ();
    type Input = PreferencesMsg;
    type Output = ();

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
            grid_sender.input(PreferencesMsg::GridSizeChanged(row.value().round() as i32));
        });

        let helpers = adw::SwitchRow::builder()
            .title(&gettext("Helpers"))
            .active(settings.boolean("helpers"))
            .build();

        let helpers_sender = sender.clone();
        helpers.connect_active_notify(move |row| {
            helpers_sender.input(PreferencesMsg::HelpersChanged(row.is_active()));
        });

        let group = adw::PreferencesGroup::builder()
            .title(&gettext("Game"))
            .build();
        group.add(&grid_size);
        group.add(&helpers);

        let page = adw::PreferencesPage::builder()
            .title(&gettext("General"))
            .icon_name("preferences-system-symbolic")
            .build();
        page.add(&group);

        root.add(&page);
        root.present(Some(&relm4::main_application().windows()[0]));

        let widgets = root.clone();
        let model = PreferencesDialog { settings };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            PreferencesMsg::GridSizeChanged(size) => {
                self.settings.set_int("grid-size", size).unwrap();
            }
            PreferencesMsg::HelpersChanged(enabled) => {
                self.settings.set_boolean("helpers", enabled).unwrap();
            }
        }
    }
}
