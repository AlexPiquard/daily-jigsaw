use adw::prelude::AdwDialogExt;
use gtk::prelude::GtkApplicationExt;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

use crate::config::{APP_ID, APP_NAME, VERSION};

pub struct AboutDialog {}

impl SimpleComponent for AboutDialog {
    type Init = ();
    type Widgets = adw::AboutDialog;
    type Input = ();
    type Output = ();
    type Root = adw::AboutDialog;

    fn init_root() -> Self::Root {
        adw::AboutDialog::builder()
            .application_icon(*APP_ID)
            .license_type(gtk::License::Gpl30)
            .website("https://github.com/AlexPiquard/daily-jigsaw")
            .issue_url("https://github.com/AlexPiquard/daily-jigsaw/issues")
            .application_name(*APP_NAME)
            .version(*VERSION)
            .translator_credits("translator-credits")
            .copyright("© 2026 Alex Piquard")
            .developers(vec!["Alex Piquard"])
            .designers(vec!["Alex Piquard"])
            .build()
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {};

        let widgets = root.clone();
        widgets.present(Some(&relm4::main_application().windows()[0]));

        ComponentParts { model, widgets }
    }

    fn update_view(&self, _dialog: &mut Self::Widgets, _sender: ComponentSender<Self>) {}
}
