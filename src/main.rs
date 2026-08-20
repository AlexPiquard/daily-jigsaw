mod app;
mod config;
mod core;
mod modals;
mod widgets;

use gettextrs::LocaleCategory;
use gtk::prelude::ApplicationExt;
use relm4::{
    RelmApp,
    gtk::{self, gio, glib},
    main_application,
};

use crate::{
    app::App,
    config::{APP_ID, APP_NAME, GETTEXT_PACKAGE, LOCALEDIR, PATH_ID, RESOURCES_FILE},
};

relm4::new_action_group!(AppActionGroup, "app");
relm4::new_stateless_action!(QuitAction, AppActionGroup, "quit");

fn main() {
    gtk::init().unwrap();

    // Enable logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "daily_jigsaw=info,relm4=warn".into()),
        )
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::FULL)
        .init();

    // setup gettext
    gettextrs::setlocale(LocaleCategory::LcAll, "");
    gettextrs::bindtextdomain(*GETTEXT_PACKAGE, *LOCALEDIR)
        .expect("Unable to bind the text domain");
    gettextrs::textdomain(*GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    glib::set_application_name(*APP_NAME);

    let res = gio::Resource::load(*RESOURCES_FILE).expect("Could not load gresource file");
    gio::resources_register(&res);

    gtk::Window::set_default_icon_name(*APP_ID);

    let app = main_application();
    app.set_resource_base_path(Some(*PATH_ID));

    let app = RelmApp::from_app(app);

    let data = res
        .lookup_data(
            format!("{}style.css", *PATH_ID).as_ref(),
            gio::ResourceLookupFlags::NONE,
        )
        .unwrap();
    relm4::set_global_css(&glib::GString::from_utf8_checked(data.to_vec()).unwrap());
    app.visible_on_activate(false).run::<App>(());
}

#[cfg(test)]
mod tests {
    #[allow(unused)]
    use super::*;

    // TODO: tests
}
