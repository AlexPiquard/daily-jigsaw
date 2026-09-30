use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::subclass::prelude::ObjectSubclassIsExt;

mod imp {
    use std::{
        cell::{OnceCell, RefCell},
        path::PathBuf,
        rc::Rc,
    };

    use anyhow::Context;
    use relm4::{
        adw::subclass::prelude::{ObjectImpl, ObjectSubclass},
        gtk::{
            self, glib,
            graphene::Size,
            prelude::*,
            subclass::{prelude::*, widget::WidgetImpl},
        },
    };

    use crate::{config::APP_ID, core::jigsaw::Jigsaw};

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

    const BOARD_MARGIN: f32 = 150.0;

    #[derive(Default)]
    pub struct BoardView {
        jigsaw: RefCell<Option<Jigsaw>>,
        source: RefCell<Option<Rc<gtk::gdk_pixbuf::Pixbuf>>>,
        drag_state: RefCell<Option<DragState>>,
        texture: RefCell<Option<gtk::gdk::Texture>>,
        on_win: OnceCell<Box<dyn Fn()>>,
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
        fn realize(&self) {
            self.parent_realize();
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if self.obj().width() <= 0 || self.obj().height() <= 0 {
                return;
            }

            if self.jigsaw.borrow().is_none() && self.source.borrow().is_some() {
                self.create_jigsaw();
            }

            let jigsaw = self.jigsaw.borrow();
            let Some(jigsaw) = jigsaw.as_ref() else {
                return;
            };

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
                jigsaw.origin_x,
                jigsaw.origin_y,
                jigsaw.width as f32,
                jigsaw.height as f32,
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

            for piece in jigsaw.pieces() {
                let path = piece.path();
                let src = piece.source_rect();

                // Build source node: texture with transform
                let sx = piece.total_size / src.width;
                let sy = piece.total_size / src.height;
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
                let bounds = gtk::graphene::Rect::new(0.0, 0.0, piece.total_size, piece.total_size);
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
    }

    impl BoardView {
        pub fn prepare(&self, on_win: impl Fn() + 'static) {
            if self.on_win.set(Box::new(on_win)).is_err() {
                return;
            }
            self.download_daily_image();
        }

        pub fn create_jigsaw(&self) {
            let grid_size = gtk::gio::Settings::new(*APP_ID)
                .int("grid-size")
                .clamp(3, 10) as usize;

            let area_w = self.obj().width() as f32;
            let area_h = self.obj().height() as f32;
            let source = self.source.borrow();
            let Some(source) = source.as_ref() else {
                return;
            };
            let img_w = source.width() as f32;
            let img_h = source.height() as f32;

            // fit the image into the available area
            let avail_w = (area_w - 2.0 * BOARD_MARGIN).max(1.0);
            let avail_h = (area_h - 2.0 * BOARD_MARGIN).max(1.0);
            let scale = (avail_w / img_w).min(avail_h / img_h).min(1.0);
            let disp_w = img_w * scale;
            let disp_h = img_h * scale;

            // center the leftover space if aspect ratios differ
            let origin_x = BOARD_MARGIN + (avail_w - disp_w) / 2.0;
            let origin_y = BOARD_MARGIN + (avail_h - disp_h) / 2.0;

            let mut jigsaw = Jigsaw::new(
                grid_size,
                disp_w.round() as i32,
                disp_h.round() as i32,
                scale,
                origin_x,
                origin_y,
            );
            jigsaw.scatter(area_w, area_h);
            *self.jigsaw.borrow_mut() = Some(jigsaw);
        }

        pub fn reset(&self) {
            self.create_jigsaw();
            self.obj().queue_draw();
        }

        fn download_daily_image(&self) {
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
                        }
                        Err(e) => {
                            tracing::error!("daily image: {e:#}");
                        }
                    }
                }
            });
        }

        fn setup_move_gesture(&self) {
            let gesture = gtk::GestureDrag::new();
            gesture.set_button(relm4::gtk::gdk::ffi::GDK_BUTTON_PRIMARY as u32);

            let this = self.obj().clone();
            gesture.connect_drag_begin(move |_gesture, start_x, start_y| {
                let imp = this.imp();
                let mut jigsaw = this.imp().jigsaw.borrow_mut();
                let Some(jigsaw) = jigsaw.as_mut() else {
                    return;
                };
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

                let mut jigsaw = this.imp().jigsaw.borrow_mut();
                let Some(jigsaw) = jigsaw.as_mut() else {
                    return;
                };

                let x = state.start_x + offset_x as f32;
                let y = state.start_y + offset_y as f32;

                this.imp().move_piece_to(jigsaw, x, y);
                this.queue_draw();
            });

            let this = self.obj().clone();
            gesture.connect_drag_end(move |_gesture, _offset_x, _offset_y| {
                let mut jigsaw = this.imp().jigsaw.borrow_mut();
                let Some(jigsaw) = jigsaw.as_mut() else {
                    return;
                };

                let pieces = jigsaw.pieces();
                let mut snap_to: Option<(f32, f32)> = None;
                if let Some(current_piece) = pieces.last() {
                    for piece in pieces.iter() {
                        let distance = (current_piece.x - piece.start_x)
                            * (current_piece.x - piece.start_x)
                            + (current_piece.y - piece.start_y) * (current_piece.y - piece.start_y);

                        if distance < jigsaw.snap_distance {
                            snap_to = Some((piece.start_x, piece.start_y));
                        }
                    }
                }
                if let Some((x, y)) = snap_to {
                    this.imp().move_piece_to(jigsaw, x, y);
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
        }
    }

    fn today() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / 86_400
    }

    fn download_daily_image_blocking() -> anyhow::Result<PathBuf> {
        let path = std::env::temp_dir().join(format!("daily-jigsaw-{}.jpg", today()));
        if path.exists() {
            return Ok(path);
        }

        let mut resp = ureq::get("https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1")
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
        let mut img = ureq::get(image_url)
            .call()
            .context("image download failed")?;
        let bytes = img
            .body_mut()
            .read_to_vec()
            .context("failed to read image body")?;
        std::fs::write(&path, bytes)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
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

    pub fn setup(&self, on_win: impl Fn() + 'static) {
        self.imp().prepare(on_win);
    }

    pub fn reset(&self) {
        self.imp().reset();
    }
}
