use relm4::{
    Component, ComponentParts, ComponentSender, Controller, SimpleComponent,
    actions::{AccelsPlus, RelmAction, RelmActionGroup},
    adw,
    gtk::{self, prelude::ButtonExt},
    main_application,
};

use gettextrs::gettext;
use gtk::prelude::{ApplicationExt, BoxExt, GtkWindowExt, OrientableExt, SettingsExt, WidgetExt};
use gtk::{gio, glib};

use crate::modals::{about::AboutDialog, shortcuts::ShortcutsDialog};
use crate::widgets::board::BoardView;
use crate::{
    config::{APP_ID, PROFILE},
    modals::preferences,
};

pub(super) struct App {
    preferences: Option<Controller<preferences::PreferencesDialog>>,
    board: BoardView,
}

#[derive(Debug)]
pub(super) enum AppMsg {
    Quit,
    OpenPreferences,
    ResetPuzzle,
}

relm4::new_action_group!(pub(super) WindowActionGroup, "win");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(pub(super) ShortcutsAction, WindowActionGroup, "show-help-overlay");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(QuitAction, WindowActionGroup, "quit");

#[relm4::component(pub)]
impl SimpleComponent for App {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type Widgets = AppWidgets;

    menu! {
        primary_menu: {
            section! {
                &gettext("_Preferences") => PreferencesAction,
                &gettext("_Shortcuts") => ShortcutsAction,
                &gettext("_About Daily Jigsaw") => AboutAction,
            }
        }
    }

    view! {
        main_window = adw::ApplicationWindow::new(&main_application()) {
            set_visible: true,

            connect_close_request[sender] => move |_| {
                sender.input(AppMsg::Quit);
                glib::Propagation::Stop
            },

            add_css_class?: if *PROFILE == "Devel" {
                    Some("devel")
                } else {
                    None
                },

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                adw::HeaderBar {
                    pack_end = &gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 12,

                        gtk::Button {
                            set_tooltip_text: Some(&gettext("Reset puzzle")),
                            add_css_class: "destructive-action",

                            connect_clicked[sender] => move |_| {
                                sender.input(AppMsg::ResetPuzzle);
                            },

                            adw::ButtonContent {
                                set_icon_name: "view-refresh-symbolic",
                                set_label: &gettext("Reset"),
                            }
                        },

                        gtk::MenuButton {
                            set_icon_name: "open-menu-symbolic",
                            set_menu_model: Some(&primary_menu),
                        }
                    }
                },

                #[name = "toast_overlay"]
                adw::ToastOverlay {
                    #[name = "content_box"]
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                    },
                },
            }

        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let widgets = view_output!();

        let board = BoardView::new();
        board.set_hexpand(true);
        board.set_vexpand(true);

        let model = Self {
            preferences: None,
            board,
        };

        let overlay = widgets.toast_overlay.clone();
        model.board.setup(move || {
            overlay.add_toast(adw::Toast::new(&gettext("Puzzle solved!")));
        });

        widgets.content_box.append(&model.board);

        let app = root.application().unwrap();
        let mut actions = RelmActionGroup::<WindowActionGroup>::new();

        let preferences_action = {
            let sender = sender.clone();
            RelmAction::<PreferencesAction>::new_stateless(move |_| {
                sender.input(AppMsg::OpenPreferences);
            })
        };

        let shortcuts_action = {
            RelmAction::<ShortcutsAction>::new_stateless(move |_| {
                ShortcutsDialog::builder().launch(()).detach();
            })
        };

        let about_action = {
            RelmAction::<AboutAction>::new_stateless(move |_| {
                AboutDialog::builder().launch(()).detach();
            })
        };

        let quit_action = {
            RelmAction::<QuitAction>::new_stateless(move |_| {
                sender.input(AppMsg::Quit);
            })
        };

        // Connect action with hotkeys
        app.set_accelerators_for_action::<QuitAction>(&["<Control>q"]);

        actions.add_action(preferences_action);
        actions.add_action(shortcuts_action);
        actions.add_action(about_action);
        actions.add_action(quit_action);
        actions.register_for_widget(&widgets.main_window);

        widgets.load_window_size();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AppMsg::Quit => main_application().quit(),
            AppMsg::OpenPreferences => {
                self.preferences = Some(
                    preferences::PreferencesDialog::builder()
                        .launch(())
                        .forward(sender.input_sender(), |output| match output {
                            preferences::PreferencesOutput::Changed => AppMsg::ResetPuzzle,
                        }),
                );
            }
            AppMsg::ResetPuzzle => {
                self.board.reset();
            }
        }
    }

    fn shutdown(&mut self, widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        widgets.save_window_size().unwrap();
        self.board.shutdown();
    }
}

impl AppWidgets {
    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = gio::Settings::new(*APP_ID);
        let (width, height) = self.main_window.default_size();

        settings.set_int("window-width", width)?;
        settings.set_int("window-height", height)?;

        settings.set_boolean("is-maximized", self.main_window.is_maximized())?;

        Ok(())
    }

    fn load_window_size(&self) {
        let settings = gio::Settings::new(*APP_ID);

        let width = settings.int("window-width");
        let height = settings.int("window-height");
        let is_maximized = settings.boolean("is-maximized");

        self.main_window.set_default_size(width, height);

        if is_maximized {
            self.main_window.maximize();
        }
    }
}
