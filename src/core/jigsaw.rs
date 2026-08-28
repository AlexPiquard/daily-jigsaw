use std::cell::OnceCell;

use rand::Rng;

pub const ARC_RATIO: f32 = 1.0 / 6.0;
const LEFT_ANGLE: f64 = std::f64::consts::PI;
const RIGHT_ANGLE: f64 = 0.0;
const TOP_ANGLE: f64 = 1.5 * std::f64::consts::PI;
const BOTTOM_ANGLE: f64 = 0.5 * std::f64::consts::PI;
const ARC_SEGMENTS: i32 = 20;

#[derive(Debug)]
pub struct Jigsaw {
    solved: bool,
    pieces: Vec<Piece>,
    pub snap_distance: f32,
    pub width: i32,
    pub height: i32,
    pub origin_x: f32,
    pub origin_y: f32,
}

impl Jigsaw {
    /// `width`/`height`: displayed puzzle size, in widget pixels.
    /// `scale`: factor from displayed pixels to source image pixels.
    /// `origin_x`/`origin_y`: grid origin within the widget.
    pub fn new(
        pieces_per_axis: usize,
        width: i32,
        height: i32,
        scale: f32,
        origin_x: f32,
        origin_y: f32,
    ) -> Self {
        let mut this = Self {
            pieces: vec![],
            width,
            height,
            origin_x,
            origin_y,
            snap_distance: 0.0,
            solved: false,
        };
        this.create_pieces(pieces_per_axis, width, height, scale, origin_x, origin_y);
        this
    }

    fn create_pieces(
        &mut self,
        pieces_per_axis: usize,
        width: i32,
        height: i32,
        scale: f32,
        origin_x: f32,
        origin_y: f32,
    ) {
        let v_pieces;
        let h_pieces;
        let piece_size;
        if width >= height {
            v_pieces = pieces_per_axis;
            piece_size = height as usize / v_pieces;
            h_pieces = width as usize / piece_size;
            self.width = piece_size as i32 * h_pieces as i32;
        } else {
            h_pieces = pieces_per_axis;
            piece_size = width as usize / h_pieces;
            v_pieces = height as usize / piece_size;
            self.height = piece_size as i32 * v_pieces as i32;
        }
        self.snap_distance = (piece_size as f32 / 3.0) * (piece_size as f32 / 3.0);

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

                self.pieces.push(Piece::new(
                    x,
                    y,
                    piece_size,
                    scale,
                    origin_x,
                    origin_y,
                    top_hole,
                    right_hole,
                    bottom_hole,
                    left_hole,
                ));
            }
        }
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
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

    pub fn scatter(&mut self, area_w: f32, area_h: f32) {
        let mut rng = rand::rng();
        for piece in &mut self.pieces {
            let max_x = (area_w - piece.total_size).max(0.0);
            let max_y = (area_h - piece.total_size).max(0.0);
            piece.x = rng.random_range(0.0..=max_x);
            piece.y = rng.random_range(0.0..=max_y);
        }
    }
}

#[derive(Debug)]
pub struct Piece {
    index_x: usize,
    index_y: usize,
    pub start_x: f32,
    pub start_y: f32,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    arc_radius: f32,
    pub total_size: f32,
    /// displayed pixels -> source image pixels
    scale: f32,

    pub top_hole: Option<bool>,
    pub right_hole: Option<bool>,
    pub bottom_hole: Option<bool>,
    pub left_hole: Option<bool>,

    path: OnceCell<relm4::gtk::gsk::Path>,
}

impl Piece {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        x: usize,
        y: usize,
        piece_size: usize,
        scale: f32,
        origin_x: f32,
        origin_y: f32,
        top_hole: Option<bool>,
        right_hole: Option<bool>,
        bottom_hole: Option<bool>,
        left_hole: Option<bool>,
    ) -> Self {
        let arc_radius = piece_size as f32 * ARC_RATIO;
        let start_x = origin_x - arc_radius + x as f32 * piece_size as f32;
        let start_y = origin_y - arc_radius + y as f32 * piece_size as f32;
        Self {
            index_x: x,
            index_y: y,
            start_x,
            start_y,
            x: start_x,
            y: start_y,
            size: piece_size as f32,
            arc_radius,
            total_size: piece_size as f32 + 2.0 * arc_radius,
            scale,
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

    /// The source crop of the full image, in image pixels.
    pub fn source_rect(&self) -> SourceRect {
        // a cell of `size` displayed pixels = `size / scale` image pixels
        let cell_img = self.size / self.scale;
        let arc_img = self.arc_radius / self.scale;
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
        builder.move_to(self.arc_radius, self.arc_radius);

        if let Some(top_hole) = self.top_hole {
            builder.line_to(self.size / 2.0 - self.arc_radius / 2.0, self.arc_radius);
            Piece::add_arc(
                &builder,
                self.size / 2.0 + self.arc_radius,
                self.arc_radius,
                self.arc_radius,
                LEFT_ANGLE,
                RIGHT_ANGLE,
                top_hole,
            );
        }
        builder.line_to(self.size + self.arc_radius, self.arc_radius);

        if let Some(right_hole) = self.right_hole {
            builder.line_to(self.size + self.arc_radius, self.size / 2.0);
            Piece::add_arc(
                &builder,
                self.arc_radius + self.size,
                self.arc_radius + self.size / 2.0,
                self.arc_radius,
                TOP_ANGLE,
                BOTTOM_ANGLE,
                right_hole,
            );
        }
        builder.line_to(self.arc_radius + self.size, self.arc_radius + self.size);

        if let Some(bottom_hole) = self.bottom_hole {
            builder.line_to(
                self.size / 2.0 + self.arc_radius * 2.0,
                self.arc_radius + self.size,
            );
            Piece::add_arc(
                &builder,
                self.arc_radius + self.size / 2.0,
                self.size + self.arc_radius,
                self.arc_radius,
                RIGHT_ANGLE,
                LEFT_ANGLE,
                bottom_hole,
            );
        }
        builder.line_to(self.arc_radius, self.arc_radius + self.size);

        if let Some(left_hole) = self.left_hole {
            builder.line_to(self.arc_radius, self.size / 2.0 + self.arc_radius * 2.0);
            Piece::add_arc(
                &builder,
                self.arc_radius,
                self.arc_radius + self.size / 2.0,
                self.arc_radius,
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
