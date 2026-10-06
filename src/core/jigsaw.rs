use std::{
    cell::{OnceCell, Ref, RefCell},
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    rc::Rc,
};

use anyhow::Context;
use rand::Rng;
use relm4::gtk::glib::{self};

use crate::config::APP_ID;

pub const ARC_RATIO: f32 = 1.0 / 6.0;
const LEFT_ANGLE: f64 = std::f64::consts::PI;
const RIGHT_ANGLE: f64 = 0.0;
const TOP_ANGLE: f64 = 1.5 * std::f64::consts::PI;
const BOTTOM_ANGLE: f64 = 0.5 * std::f64::consts::PI;
const ARC_SEGMENTS: i32 = 20;
const BOARD_MARGIN: f32 = 150.0;
const MIN_PIECE_SIZE: f32 = 50.0;

pub struct BoardMetrics {
    pub grid_size: usize,
    pub area_w: f32,
    pub area_h: f32,
    pub img_w: f32,
    pub img_h: f32,
}

/// `width`/`height`: displayed puzzle size, in widget pixels.
/// `area_w`/`area_h`: board size.
/// `scale`: factor from displayed pixels to source image pixels.
/// `origin_x`/`origin_y`: grid origin within the widget.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Metrics {
    pub snap_distance: f32,
    pub width: i32,
    pub height: i32,
    pub area_w: f32,
    pub area_h: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub piece_size: f32,
    pub h_pieces: usize,
    pub v_pieces: usize,
    /// displayed pixels -> source image pixels
    pub scale: f32,
    pub grid_size: usize,
}

/// The widget is too small to be cut into playable pieces.
#[derive(Debug, thiserror::Error)]
#[error("board too small to hold the puzzle")]
pub struct BoardTooSmall;

impl Metrics {
    pub fn total_piece_size(&self) -> f32 {
        self.piece_size + 2.0 * self.piece_arc_radius()
    }

    pub fn piece_arc_radius(&self) -> f32 {
        self.piece_size * ARC_RATIO
    }

    /// `Err(BoardTooSmall)` when the area can't be cut into pieces of at least one
    /// pixel, which would make the piece counts degenerate.
    pub fn try_new(board_metrics: BoardMetrics) -> Result<Self, BoardTooSmall> {
        let mut metrics = Metrics {
            area_w: board_metrics.area_w,
            area_h: board_metrics.area_h,
            grid_size: board_metrics.grid_size,
            ..Default::default()
        };

        metrics.reset(board_metrics)?;

        Ok(metrics)
    }

    fn reset(&mut self, board_metrics: BoardMetrics) -> Result<(), BoardTooSmall> {
        let BoardMetrics {
            grid_size,
            area_w,
            area_h,
            img_w,
            img_h,
        } = board_metrics;

        if grid_size == 0 || img_w <= 0.0 || img_h <= 0.0 {
            return Err(BoardTooSmall);
        }

        // fit the image into the available area
        let avail_w = (area_w - 2.0 * BOARD_MARGIN).max(1.0);
        let avail_h = (area_h - 2.0 * BOARD_MARGIN).max(1.0);
        let scale = (avail_w / img_w).min(avail_h / img_h).min(1.0);
        let disp_w = img_w * scale;
        let disp_h = img_h * scale;

        // center the leftover space if aspect ratios differ
        let origin_x = BOARD_MARGIN + (avail_w - disp_w) / 2.0;
        let origin_y = BOARD_MARGIN + (avail_h - disp_h) / 2.0;

        let v_pieces;
        let h_pieces;
        let piece_size: f32;
        let mut width = disp_w as i32;
        let mut height = disp_h as i32;
        if disp_w >= disp_h {
            v_pieces = grid_size;
            piece_size = disp_h / v_pieces as f32;
            h_pieces = (disp_w / piece_size) as usize;
            width = piece_size as i32 * h_pieces as i32;
        } else {
            h_pieces = grid_size;
            piece_size = disp_w / h_pieces as f32;
            v_pieces = (disp_h / piece_size) as usize;
            height = piece_size as i32 * v_pieces as i32;
        }

        if piece_size < MIN_PIECE_SIZE || h_pieces == 0 || v_pieces == 0 {
            return Err(BoardTooSmall);
        }

        self.snap_distance = (piece_size / 2.0) * (piece_size / 2.0);
        self.width = width;
        self.height = height;
        self.area_w = area_w;
        self.area_h = area_h;
        self.origin_x = origin_x;
        self.origin_y = origin_y;
        self.piece_size = piece_size;
        self.h_pieces = h_pieces;
        self.v_pieces = v_pieces;
        self.scale = scale;
        self.grid_size = grid_size;

        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(into = "JigsawDto", try_from = "JigsawDto")]
pub struct Jigsaw {
    #[serde(skip)]
    solved: bool,
    pieces: Vec<Piece>,
    #[serde(skip)]
    metrics: RefCell<Rc<Metrics>>,
    path: PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct JigsawDto {
    pieces: Vec<Piece>,
    metrics: Metrics,
    path: PathBuf,
}

impl From<Jigsaw> for JigsawDto {
    fn from(val: Jigsaw) -> Self {
        let metrics = (**val.metrics()).clone();
        JigsawDto {
            pieces: val.pieces,
            metrics,
            path: val.path,
        }
    }
}

impl TryFrom<JigsawDto> for Jigsaw {
    type Error = anyhow::Error;
    fn try_from(dto: JigsawDto) -> Result<Self, Self::Error> {
        let metrics = RefCell::new(Rc::new(dto.metrics));
        Ok(Jigsaw {
            pieces: dto.pieces,
            metrics,
            path: dto.path,
            solved: false,
        })
    }
}

impl Jigsaw {
    pub fn new(metrics: Metrics, path: PathBuf) -> Self {
        let mut this = Self {
            pieces: vec![],
            solved: false,
            metrics: RefCell::new(Rc::new(metrics)),
            path,
        };
        this.create_pieces();
        this
    }

    fn create_pieces(&mut self) {
        let v_pieces = self.metrics().v_pieces;
        let h_pieces = self.metrics().h_pieces;
        // bitwise for right & bottom holes in pieces at y, x
        // bit 0 = bottom hole, bit 1 = right hole
        let mut holes: Vec<Vec<u8>> = vec![vec![0u8; h_pieces]; v_pieces];

        for y in 0..v_pieces {
            for x in 0..h_pieces {
                let top_hole: Option<bool> = if y > 0 {
                    Some((holes[y - 1][x] & 1) == 0)
                } else {
                    None
                };
                let left_hole: Option<bool> = if x > 0 {
                    Some((holes[y][x - 1] & 2) == 0)
                } else {
                    None
                };
                let bottom_hole: Option<bool> = if y < v_pieces - 1 {
                    Some(rand::random())
                } else {
                    None
                };
                let right_hole: Option<bool> = if x < h_pieces - 1 {
                    Some(rand::random())
                } else {
                    None
                };

                // interlocking grid: only 2 bits (bottom + right)
                holes[y][x] = (bottom_hole.unwrap_or(false) as u8)
                    | ((right_hole.unwrap_or(false) as u8) << 1);

                let metrics = self.metrics().clone();
                self.pieces.push(Piece::new(
                    metrics,
                    x,
                    y,
                    top_hole,
                    right_hole,
                    bottom_hole,
                    left_hole,
                ));
            }
        }
    }

    pub fn metrics(&self) -> Ref<'_, Rc<Metrics>> {
        self.metrics.borrow()
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub fn path_eq(&self, path: &Path) -> bool {
        self.path.eq(path)
    }

    pub fn is_solved(&self) -> bool {
        self.solved
    }

    pub fn move_on_top(&mut self, index: usize) {
        let piece = self.pieces.remove(index);
        self.pieces.push(piece);
    }

    pub fn move_last_to(&mut self, x: f32, y: f32) -> bool {
        if let Some(piece) = self.pieces.last_mut() {
            piece.x = x;
            piece.y = y;

            return x == piece.start_x && y == piece.start_y && self.check_solved();
        }

        false
    }

    pub fn check_solved(&mut self) -> bool {
        if self.solved {
            return false;
        }
        self.solved = self.pieces.iter().all(|p| p.is_in_place());
        self.solved
    }

    pub fn scatter(&mut self) {
        let mut rng = rand::rng();
        for i in 0..self.pieces.len() {
            let (mut min_x, mut max_x, mut min_y, mut max_y) = self.piece_area(i);
            if min_x >= max_x || min_y >= max_y {
                (min_x, max_x, min_y, max_y) = self.piece_area(i + 1);
            }
            let piece = self.pieces.get_mut(i).unwrap();
            piece.x = rng.random_range(min_x..=max_x.max(0.0));
            piece.y = rng.random_range(min_y..=max_y.max(0.0));
        }
    }

    pub fn piece_area(&self, side_index: usize) -> (f32, f32, f32, f32) {
        let metrics = self.metrics().clone();
        let total_piece_size = metrics.total_piece_size();
        let mut min_x = 0.0;
        let mut max_x = metrics.area_w - total_piece_size;
        let mut min_y = 0.0;
        let mut max_y = metrics.area_h - total_piece_size;
        match side_index % 4 {
            0 => {
                max_x = metrics.origin_x - total_piece_size;
            }
            1 => {
                max_y = metrics.origin_y - total_piece_size;
            }
            2 => {
                min_x = metrics.origin_x + metrics.width as f32;
            }
            3 => {
                min_y = metrics.origin_y + metrics.height as f32;
            }
            _ => unreachable!(),
        }
        (min_x, max_x, min_y, max_y)
    }

    pub fn reset(&mut self, board_metrics: BoardMetrics) -> anyhow::Result<()> {
        let mut metrics = (**self.metrics()).clone();
        metrics.reset(board_metrics)?;
        *self.metrics.borrow_mut() = Rc::new(metrics);
        self.pieces.clear();
        self.create_pieces();
        self.scatter();
        Ok(())
    }

    fn state_file() -> anyhow::Result<PathBuf> {
        let dir = glib::user_state_dir().join(*APP_ID);
        std::fs::create_dir_all(&dir).context("failed to create state dir")?;
        Ok(dir.join("state.json"))
    }

    pub fn delete_state(state_file: &Path) -> anyhow::Result<()> {
        match std::fs::remove_file(state_file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("failed to remove {}", state_file.display())),
        }
    }

    pub fn write_state(&self) -> anyhow::Result<()> {
        let state_file = Self::state_file()?;

        if self.is_solved() {
            return Self::delete_state(&state_file);
        }

        let json = serde_json::to_string(self).context("failed to stringify jigsaw state")?;
        let tmp_file = state_file.with_extension("json.tmp");
        std::fs::write(&tmp_file, json)
            .with_context(|| format!("failed to write {}", tmp_file.display()))?;
        // rename is atomic: a crash while writing can't leave a truncated state behind
        std::fs::rename(&tmp_file, &state_file)
            .with_context(|| format!("failed to rename {}", tmp_file.display()))?;

        Ok(())
    }

    fn load_state(image_path: &Path, grid_size: usize) -> anyhow::Result<Option<Jigsaw>> {
        let state_file = Self::state_file()?;
        if !state_file.try_exists()? {
            return Ok(None);
        }

        let file = File::open(state_file)?;

        let reader = BufReader::new(file);
        let jigsaw: Jigsaw = serde_json::from_reader(reader)?;

        if !jigsaw.path_eq(image_path) {
            return Ok(None);
        }

        if jigsaw.metrics().grid_size != grid_size {
            tracing::debug!("saved state has another grid, starting a new puzzle");
            return Ok(None);
        }

        for piece in jigsaw.pieces.iter() {
            piece.set_metrics(jigsaw.metrics().clone());
        }

        Ok(Some(jigsaw))
    }

    pub fn load_or_create_jigsaw(
        image_path: PathBuf,
        board_metrics: BoardMetrics,
    ) -> anyhow::Result<Jigsaw> {
        let jigsaw = match Self::load_state(&image_path, board_metrics.grid_size) {
            Ok(Some(jigsaw)) => Some(jigsaw),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("failed to load jigsaw state: {e:#}");
                None
            }
        };

        let jigsaw = jigsaw
            .map(Ok)
            .unwrap_or_else(|| -> anyhow::Result<Jigsaw> {
                let metrics = Metrics::try_new(board_metrics)?;
                let mut jigsaw = Jigsaw::new(metrics, image_path);
                jigsaw.scatter();
                Ok(jigsaw)
            })?;

        Ok(jigsaw)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Piece {
    #[serde(skip)]
    metrics: OnceCell<Rc<Metrics>>,

    index_x: usize,
    index_y: usize,
    pub start_x: f32,
    pub start_y: f32,
    pub x: f32,
    pub y: f32,

    pub top_hole: Option<bool>,
    pub right_hole: Option<bool>,
    pub bottom_hole: Option<bool>,
    pub left_hole: Option<bool>,

    #[serde(skip)]
    path: OnceCell<relm4::gtk::gsk::Path>,
}

impl Piece {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metrics: Rc<Metrics>,
        index_x: usize,
        index_y: usize,
        top_hole: Option<bool>,
        right_hole: Option<bool>,
        bottom_hole: Option<bool>,
        left_hole: Option<bool>,
    ) -> Self {
        let arc_radius = metrics.piece_arc_radius();
        let start_x = metrics.origin_x - arc_radius + index_x as f32 * metrics.piece_size;
        let start_y = metrics.origin_y - arc_radius + index_y as f32 * metrics.piece_size;
        Self {
            metrics: OnceCell::from(metrics),
            index_x,
            index_y,
            start_x,
            start_y,
            x: start_x,
            y: start_y,
            top_hole,
            right_hole,
            bottom_hole,
            left_hole,
            path: OnceCell::new(),
        }
    }

    /// Returns true if the piece is correctly placed
    pub fn is_in_place(&self) -> bool {
        self.x == self.start_x && self.y == self.start_y
    }

    fn metrics(&self) -> &Rc<Metrics> {
        self.metrics.get().unwrap()
    }

    pub fn set_metrics(&self, metrics: Rc<Metrics>) {
        if self.metrics.get().is_none() {
            let _ = self.metrics.set(metrics);
        }
    }

    /// The source crop of the full image, in image pixels.
    pub fn source_rect(&self) -> SourceRect {
        // a cell of `size` displayed pixels = `size / scale` image pixels
        let cell_img = self.metrics().piece_size / self.metrics().scale;
        let arc_img = self.metrics().piece_arc_radius() / self.metrics().scale;
        SourceRect {
            x: self.index_x as f32 * cell_img - arc_img,
            y: self.index_y as f32 * cell_img - arc_img,
            width: cell_img + 2.0 * arc_img,
            height: cell_img + 2.0 * arc_img,
        }
    }

    pub fn path(&self) -> &relm4::gtk::gsk::Path {
        self.path.get_or_init(|| self.build_gsk_path())
    }

    fn build_gsk_path(&self) -> relm4::gtk::gsk::Path {
        let builder = relm4::gtk::gsk::PathBuilder::new();
        let arc_radius = self.metrics().piece_arc_radius();
        let piece_size = self.metrics().piece_size;
        builder.move_to(arc_radius, arc_radius);

        if let Some(top_hole) = self.top_hole {
            builder.line_to(piece_size / 2.0 - arc_radius / 2.0, arc_radius);
            Piece::add_arc(
                &builder,
                piece_size / 2.0 + arc_radius,
                arc_radius,
                arc_radius,
                LEFT_ANGLE,
                RIGHT_ANGLE,
                top_hole,
            );
        }
        builder.line_to(piece_size + arc_radius, arc_radius);

        if let Some(right_hole) = self.right_hole {
            builder.line_to(piece_size + arc_radius, piece_size / 2.0);
            Piece::add_arc(
                &builder,
                arc_radius + piece_size,
                arc_radius + piece_size / 2.0,
                arc_radius,
                TOP_ANGLE,
                BOTTOM_ANGLE,
                right_hole,
            );
        }
        builder.line_to(arc_radius + piece_size, arc_radius + piece_size);

        if let Some(bottom_hole) = self.bottom_hole {
            builder.line_to(piece_size / 2.0 + arc_radius * 2.0, arc_radius + piece_size);
            Piece::add_arc(
                &builder,
                arc_radius + piece_size / 2.0,
                piece_size + arc_radius,
                arc_radius,
                RIGHT_ANGLE,
                LEFT_ANGLE,
                bottom_hole,
            );
        }
        builder.line_to(arc_radius, arc_radius + piece_size);

        if let Some(left_hole) = self.left_hole {
            builder.line_to(arc_radius, piece_size / 2.0 + arc_radius * 2.0);
            Piece::add_arc(
                &builder,
                arc_radius,
                arc_radius + piece_size / 2.0,
                arc_radius,
                BOTTOM_ANGLE,
                TOP_ANGLE,
                left_hole,
            );
        }

        builder.close();
        builder.to_path()
    }

    fn add_arc(
        builder: &relm4::gtk::gsk::PathBuilder,
        cx: f32,
        cy: f32,
        radius: f32,
        angle1: f64,
        angle2: f64,
        hole: bool,
    ) {
        let start_x = cx as f64 + radius as f64 * angle1.cos();
        let start_y = cy as f64 + radius as f64 * angle1.sin();
        builder.line_to(start_x as f32, start_y as f32);

        let mut diff = angle2 - angle1;
        if hole {
            if diff > 0.0 {
                diff -= 2.0 * std::f64::consts::PI;
            }
        } else {
            if diff < 0.0 {
                diff += 2.0 * std::f64::consts::PI;
            }
        }

        for i in 1..=ARC_SEGMENTS {
            let t = i as f64 / ARC_SEGMENTS as f64;
            let angle = angle1 + t * diff;
            let x = cx as f64 + radius as f64 * angle.cos();
            let y = cy as f64 + radius as f64 * angle.sin();
            builder.line_to(x as f32, y as f32);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SourceRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
