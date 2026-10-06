use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::subclass::prelude::ObjectSubclassIsExt;

mod imp {
    use std::{
        cell::{Cell, OnceCell, Ref, RefCell, RefMut},
        path::{Path, PathBuf},
        rc::Rc,
        time::{Duration, SystemTime},
    };

    use anyhow::Context;
    use gettextrs::gettext;
    use relm4::{
        adw::subclass::prelude::{ObjectImpl, ObjectSubclass},
        gtk::{
            self, glib,
            graphene::Size,
            prelude::*,
            subclass::{prelude::*, widget::WidgetImpl},
        },
    };

    use crate::{
        config::APP_ID,
        core::jigsaw::{BoardMetrics, BoardTooSmall, Jigsaw},
    };

    #[derive(serde::Deserialize)]
    struct BingImageResponse {
        url: String,
    }

    impl BingImageResponse {
        fn image_url(&self) -> String {
            format!("https://bing.com{}", self.url)
        }
    }

    #[derive(serde::Deserialize)]
    struct BingResponse {
        images: Vec<BingImageResponse>,
    }

    struct DragState {
        start_x: f32,
        start_y: f32,
    }

    #[derive(Default)]
    pub struct BoardView {
        jigsaw: RefCell<Option<Jigsaw>>,
        source: RefCell<Option<Rc<gtk::gdk_pixbuf::Pixbuf>>>,
        drag_state: RefCell<Option<DragState>>,
        texture: RefCell<Option<gtk::gdk::Texture>>,
        on_win: OnceCell<Box<dyn Fn()>>,
        on_error: OnceCell<Box<dyn Fn(String, bool)>>,
        downloading: Rc<Cell<bool>>,
        path: RefCell<Option<PathBuf>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BoardView {
        const NAME: &'static str = "BoardView";
        type Type = super::BoardView;
        type ParentType = gtk::Widget;

        fn new() -> Self {
            Self::default()
        }
    }

    impl ObjectImpl for BoardView {
        fn constructed(&self) {
            self.parent_constructed();
            self.setup_move_gesture();
        }
    }

    impl WidgetImpl for BoardView {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if self.obj().width() <= 0 || self.obj().height() <= 0 {
                return;
            }

            let jigsaw = self.jigsaw.borrow();
            let Some(jigsaw) = jigsaw.as_ref() else {
                return;
            };
            let metrics = jigsaw.metrics();

            let source = self.source.borrow();
            let Some(source) = source.as_ref() else {
                return;
            };

            let texture = self.texture.borrow();
            let Some(texture) = texture.as_ref() else {
                return;
            };

            let black = gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0);
            let bounds = gtk::graphene::Rect::new(
                metrics.origin_x,
                metrics.origin_y,
                metrics.width as f32,
                metrics.height as f32,
            );
            let rounded = gtk::gsk::RoundedRect::new(
                bounds,
                Size::zero(),
                Size::zero(),
                Size::zero(),
                Size::zero(),
            );
            let border = gtk::gsk::BorderNode::new(
                &rounded,
                &[1.0, 1.0, 1.0, 1.0],
                &[black, black, black, black],
            );
            snapshot.append_node(&border);

            let total_piece_size = metrics.total_piece_size();
            for piece in jigsaw.pieces() {
                let path = piece.path();
                let src = piece.source_rect();

                // Build source node: texture with transform
                let sx = total_piece_size / src.width;
                let sy = total_piece_size / src.height;
                let tx = -src.x * sx;
                let ty = -src.y * sy;
                let matrix = gtk::graphene::Matrix::from_2d(
                    sx as f64, 0.0, 0.0, sy as f64, tx as f64, ty as f64,
                );
                let img_rect = gtk::graphene::Rect::new(
                    0.0,
                    0.0,
                    source.width() as f32,
                    source.height() as f32,
                );
                let tex_node = gtk::gsk::TextureNode::new(texture, &img_rect);
                let transform = gtk::gsk::Transform::new().matrix(&matrix);
                let source_node = gtk::gsk::TransformNode::new(&tex_node, Some(&transform));

                // Build mask node: white fill of piece path
                let white = gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
                let bounds = gtk::graphene::Rect::new(0.0, 0.0, total_piece_size, total_piece_size);
                let color_node = gtk::gsk::ColorNode::new(&white, &bounds);
                let fill_node =
                    gtk::gsk::FillNode::new(&color_node, path, gtk::gsk::FillRule::Winding);

                // Combine: source masked by piece shape
                let mask_node =
                    gtk::gsk::MaskNode::new(&source_node, &fill_node, gtk::gsk::MaskMode::Alpha);

                // Translate to piece position
                let pos = gtk::graphene::Point::new(piece.x, piece.y);
                let translate = gtk::gsk::Transform::new().translate(&pos);
                let translated = gtk::gsk::TransformNode::new(&mask_node, Some(&translate));
                snapshot.append_node(&translated);
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            if let Err(e) = self.try_create_jigsaw()
                && let Some(on_error) = self.on_error.get()
            {
                tracing::error!("create jigsaw: {e:#}");
                on_error(gettext("Couldn't create jigsaw"), true);
            }
        }
    }

    impl BoardView {
        pub fn prepare(
            &self,
            on_win: impl Fn() + 'static,
            on_error: impl Fn(String, bool) + 'static,
        ) {
            if self.on_win.set(Box::new(on_win)).is_err() {
                return;
            }
            if self.on_error.set(Box::new(on_error)).is_err() {
                return;
            }

            self.download_daily_image();
        }

        fn jigsaw(&self) -> Ref<'_, Jigsaw> {
            Ref::map(self.jigsaw.borrow(), |j| {
                j.as_ref().expect("jisgaw should be defined")
            })
        }

        fn jigsaw_mut(&self) -> RefMut<'_, Jigsaw> {
            RefMut::map(self.jigsaw.borrow_mut(), |j| {
                j.as_mut().expect("jisgaw should be defined")
            })
        }

        pub fn shutdown(&self) {
            if self.jigsaw.borrow().is_some()
                && let Err(e) = self.jigsaw().write_state()
            {
                tracing::error!("board shutdown: {e:#}");
            }
        }

        pub fn try_create_jigsaw(&self) -> anyhow::Result<()> {
            let board_metrics = self.board_metrics()?;

            if self.jigsaw.borrow().is_some()
                || self.source.borrow().is_none()
                || board_metrics.area_w <= 0.0
                || board_metrics.area_h <= 0.0
            {
                return Ok(());
            }

            let path = self.path.borrow();
            let Some(path) = path.as_ref() else {
                anyhow::bail!("image path is not defined");
            };
            let jigsaw: Option<Jigsaw> =
                match Jigsaw::load_or_create_jigsaw(path.clone(), board_metrics) {
                    Ok(jigsaw) => Some(jigsaw),
                    // too small to hold the puzzle: stay quiet, `size_allocate` will
                    // try again once the widget is big enough
                    Err(e) if e.downcast_ref::<BoardTooSmall>().is_some() => None,
                    Err(e) => {
                        return Err(e);
                    }
                };

            *self.jigsaw.borrow_mut() = jigsaw;
            Ok(())
        }

        fn grid_size(&self) -> usize {
            gtk::gio::Settings::new(*APP_ID)
                .int("grid-size")
                .clamp(3, 10) as usize
        }

        fn board_metrics(&self) -> anyhow::Result<BoardMetrics> {
            let grid_size = self.grid_size();

            let area_w = self.obj().width() as f32;
            let area_h = self.obj().height() as f32;
            let source = self.source.borrow();
            let source = source.as_ref().context("no source")?;
            let img_w = source.width() as f32;
            let img_h = source.height() as f32;

            Ok(BoardMetrics {
                grid_size,
                area_w,
                area_h,
                img_w,
                img_h,
            })
        }

        pub fn reset(&self) -> anyhow::Result<()> {
            self.jigsaw_mut().reset(self.board_metrics()?)?;
            self.obj().queue_draw();
            Ok(())
        }

        pub fn download_daily_image(&self) {
            if self.downloading.replace(true) {
                return;
            }
            let downloading = self.downloading.clone();

            let (tx, rx) = flume::unbounded::<Result<PathBuf, String>>();
            std::thread::spawn(move || {
                let _ = tx.send(download_daily_image_blocking().map_err(|e| format!("{e:#}")));
            });

            let this = self.obj().clone();
            glib::spawn_future_local(async move {
                while let Ok(res) = rx.recv_async().await {
                    match res {
                        Ok(path) => {
                            this.imp().set_source(path);
                            this.queue_draw();

                            if let Err(e) = this.imp().try_create_jigsaw()
                                && let Some(on_error) = this.imp().on_error.get()
                            {
                                tracing::error!("create jigsaw: {e:#}");
                                on_error(gettext("Couldn't create jigsaw"), true);
                            }
                        }
                        Err(e) => {
                            tracing::error!("daily image: {e:#}");
                            if let Some(on_error) = this.imp().on_error.get() {
                                on_error(gettext("Couldn't download today's image"), true);
                            }
                        }
                    }
                }
                downloading.set(false);
            });
        }

        fn setup_move_gesture(&self) {
            let gesture = gtk::GestureDrag::new();
            gesture.set_button(relm4::gtk::gdk::ffi::GDK_BUTTON_PRIMARY as u32);

            let this = self.obj().clone();
            gesture.connect_drag_begin(move |_gesture, start_x, start_y| {
                let imp = this.imp();
                let mut jigsaw = this.imp().jigsaw_mut();
                let pieces = jigsaw.pieces();

                // Hit test: find topmost piece under the cursor (reverse order = top first)
                let mut hit_index = None;
                for (i, piece) in pieces.iter().enumerate().rev() {
                    let path = piece.path();
                    let point = gtk::graphene::Point::new(
                        start_x as f32 - piece.x,
                        start_y as f32 - piece.y,
                    );
                    if path.in_fill(&point, gtk::gsk::FillRule::Winding) {
                        hit_index = Some(i);
                        break;
                    }
                }

                if let Some(index) = hit_index {
                    let ps = &pieces[index];
                    let state = DragState {
                        start_x: ps.x,
                        start_y: ps.y,
                    };
                    *imp.drag_state.borrow_mut() = Some(state);

                    // Move piece to end (top z-order)
                    jigsaw.move_on_top(index);
                }
            });

            let this = self.obj().clone();
            gesture.connect_drag_update(move |_gesture, offset_x, offset_y| {
                let drag_state = this.imp().drag_state.borrow();
                let Some(state) = drag_state.as_ref() else {
                    return;
                };

                let mut jigsaw = this.imp().jigsaw_mut();

                let x = state.start_x + offset_x as f32;
                let y = state.start_y + offset_y as f32;

                this.imp().move_piece_to(&mut jigsaw, x, y);
                this.queue_draw();
            });

            let this = self.obj().clone();
            gesture.connect_drag_end(move |_gesture, _offset_x, _offset_y| {
                let mut jigsaw = this.imp().jigsaw_mut();

                let pieces = jigsaw.pieces();
                let mut snap_to: Option<(f32, f32)> = None;
                if let Some(current_piece) = pieces.last() {
                    for piece in pieces.iter() {
                        let distance = (current_piece.x - piece.start_x)
                            * (current_piece.x - piece.start_x)
                            + (current_piece.y - piece.start_y) * (current_piece.y - piece.start_y);

                        if distance < jigsaw.metrics().snap_distance {
                            snap_to = Some((piece.start_x, piece.start_y));
                        }
                    }
                }
                if let Some((x, y)) = snap_to {
                    this.imp().move_piece_to(&mut jigsaw, x, y);
                    this.queue_draw();
                }
                *this.imp().drag_state.borrow_mut() = None;
            });

            self.obj().add_controller(gesture);
        }

        fn move_piece_to(&self, jigsaw: &mut Jigsaw, x: f32, y: f32) {
            if jigsaw.move_last_to(x, y)
                && let Some(callback) = self.on_win.get()
            {
                callback();
            }
        }

        pub fn set_source(&self, path: PathBuf) {
            let Ok(source) = gtk::gdk_pixbuf::Pixbuf::from_file(&path) else {
                tracing::error!("failed to load image {}", path.display());
                return;
            };
            let source = Rc::new(source);
            *self.texture.borrow_mut() = Some(gtk::gdk::Texture::for_pixbuf(&source));
            *self.source.borrow_mut() = Some(source);
            *self.path.borrow_mut() = Some(path);
        }
    }

    fn today() -> u64 {
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / 86_400
    }

    fn image_cache_path() -> anyhow::Result<PathBuf> {
        let dir = glib::user_cache_dir().join(*APP_ID);
        std::fs::create_dir_all(&dir).context("failed to create cache dir")?;
        let path = dir.join(format!("daily-jigsaw-{}.jpg", today()));
        if let Err(e) = prune_image_cache(&dir, &path) {
            tracing::warn!("failed to prune image cache: {e:#}");
        }
        Ok(path)
    }

    fn prune_image_cache(dir: &Path, today: &Path) -> anyhow::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path == today {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with("daily-jigsaw-") || !name.ends_with(".jpg") {
                continue;
            }
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }

        Ok(())
    }

    /// Number of tries for a download before giving up.
    const DOWNLOAD_ATTEMPTS: u32 = 3;

    fn download_daily_image_blocking() -> anyhow::Result<PathBuf> {
        let path = image_cache_path()?;
        if path.exists() {
            return Ok(path);
        }

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .user_agent(concat!("daily-jigsaw/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();

        let mut last_err = None;
        for attempt in 1..=DOWNLOAD_ATTEMPTS {
            match fetch_daily_image(&agent) {
                Ok(bytes) => {
                    std::fs::write(&path, bytes)
                        .with_context(|| format!("failed to write {}", path.display()))?;
                    return Ok(path);
                }
                Err(e) => {
                    tracing::warn!(
                        "daily image download failed \
                         (attempt {attempt}/{DOWNLOAD_ATTEMPTS}): {e:#}"
                    );
                    last_err = Some(e);
                    if attempt < DOWNLOAD_ATTEMPTS {
                        std::thread::sleep(Duration::from_millis(500 * u64::from(attempt)));
                    }
                }
            }
        }

        Err(last_err.expect("error should be defined"))
    }

    fn fetch_daily_image(agent: &ureq::Agent) -> anyhow::Result<Vec<u8>> {
        let mut resp = agent
            .get("https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1")
            .call()
            .context("image request failed")?;
        let json = resp
            .body_mut()
            .read_json::<BingResponse>()
            .context("failed to read api response")?;
        let image_url = json
            .images
            .first()
            .context("invalid api response")?
            .image_url();
        let mut img = agent
            .get(image_url)
            .call()
            .context("image download failed")?;
        img.body_mut()
            .read_to_vec()
            .context("failed to read image body")
    }
}

glib::wrapper! {
    pub struct BoardView(ObjectSubclass<imp::BoardView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl BoardView {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn setup(&self, on_win: impl Fn() + 'static, on_error: impl Fn(String, bool) + 'static) {
        self.imp().prepare(on_win, on_error);
    }

    pub fn retry_download(&self) {
        self.imp().download_daily_image();
    }

    pub fn reset(&self) {
        if let Err(e) = self.imp().reset() {
            tracing::error!("failed to reset puzzle: {e:#}");
        }
    }

    pub fn shutdown(&self) {
        self.imp().shutdown();
    }
}
