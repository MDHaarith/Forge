//! Antenna arms and feed openings ported from @tscircuit/core 0.0.2132 (MIT).
//! Copyright (c) 2024 tscircuit Inc. See licenses/tscircuit-MIT.txt.
use super::route::Point;
use super::{Footprint, Pad};

pub struct Geometry {
    pub width: f64,
    pub route: Vec<Point>,
    pub secondary: Option<Point>,
    pub ground: bool,
    left_opening: bool,
}

pub fn geometry(shape: &str) -> Result<Geometry, String> {
    let (width, route, secondary, ground, left_opening) = match shape {
        "2.4ghz_quarter_wave_monopole" => (0.6, vec![[0., 0.], [31., 0.]], None, false, true),
        "2.4ghz_meandered_monopole" => (
            0.5,
            vec![
                [0., 0.],
                [0., 6.],
                [3., 6.],
                [3., 1.5],
                [6., 1.5],
                [6., 6.],
                [9., 6.],
                [9., 1.5],
                [12., 1.5],
                [12., 6.],
                [15., 6.],
            ],
            None,
            false,
            false,
        ),
        "2.4ghz_inverted_f" => (
            0.8,
            vec![
                [-4., 0.],
                [-4., 3.],
                [0., 3.],
                [0., 0.],
                [0., 3.],
                [-4., 3.],
                [-4., 8.],
                [22., 8.],
            ],
            Some([-4., 0.]),
            true,
            false,
        ),
        "2.4ghz_meandered_inverted_f" => (
            0.5,
            vec![
                [-1.7, 0.],
                [-1.7, 4.9],
                [0., 4.9],
                [0., 0.],
                [0., 4.9],
                [-1.7, 4.9],
                [2.3, 4.9],
                [2.3, 1.4],
                [5.3, 1.4],
                [5.3, 4.9],
                [8.3, 4.9],
                [8.3, 1.4],
                [11.3, 1.4],
                [11.3, 4.9],
                [13.3, 4.9],
            ],
            Some([-1.7, 0.]),
            true,
            false,
        ),
        "2.4ghz_folded_dipole" => (
            0.9,
            vec![
                [0., 0.],
                [-22.4, 0.],
                [-22.4, 9.],
                [23.6, 9.],
                [23.6, 0.],
                [1.2, 0.],
            ],
            Some([1.2, 0.]),
            false,
            false,
        ),
        _ => return Err(format!("unknown antenna shape {shape}")),
    };
    Ok(Geometry {
        width,
        route,
        secondary,
        ground,
        left_opening,
    })
}

impl Geometry {
    pub fn footprint(&self) -> Footprint {
        let mut pads = vec![Pad {
            number: "1".into(),
            at: [0.; 2],
            size: [0.8; 2],
            drill: None,
            round: false,
            radius: 0.2,
            plated: true,
        }];
        if let Some([x, y]) = self.secondary {
            pads.push(Pad {
                number: "2".into(),
                at: [x, -y],
                size: if self.ground { [0.7; 2] } else { [0.8; 2] },
                drill: self.ground.then_some([0.3, 0.3]),
                round: self.ground,
                radius: if self.ground { 0. } else { 0.2 },
                plated: true,
            });
        }
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for &[x, y] in &self.route {
            let radius = (self.width / 2.).max(0.4);
            bounds[0] = bounds[0].min(x - radius);
            bounds[1] = bounds[1].min(-y - radius);
            bounds[2] = bounds[2].max(x + radius);
            bounds[3] = bounds[3].max(-y + radius);
        }
        Footprint {
            pads,
            courtyard: bounds,
            height: 0.,
            body: bounds,
            silkscreen: vec![],
            pin1: [0.; 2],
        }
    }

    pub fn keepouts(&self) -> Vec<(Point, Point)> {
        let clearance = self.width / 2. + 1.;
        let left = self
            .route
            .iter()
            .map(|p| p[0])
            .fold(f64::INFINITY, f64::min)
            - clearance;
        let right = self
            .route
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max)
            + clearance;
        let bottom = self
            .route
            .iter()
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min)
            - clearance;
        let top = self
            .route
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max)
            + clearance;
        let [x, y] = self.secondary.unwrap_or([0.; 2]);
        let ol = x.min(0.) - 1.;
        let or = x.max(0.) + 1.;
        let ob = y.min(0.) - 1.;
        let ot = y.max(0.) + 1.;
        let rects = if self.left_opening {
            vec![
                [left, right, ot, top],
                [left, right, bottom, ob],
                [or, right, ob, ot],
            ]
        } else {
            vec![
                [left, ol, bottom, ot],
                [or, right, bottom, ot],
                [left, right, ot, top],
            ]
        };
        rects
            .into_iter()
            .filter(|r| r[1] > r[0] && r[3] > r[2])
            .map(|r| {
                (
                    [(r[0] + r[1]) / 2., (r[2] + r[3]) / 2.],
                    [r[1] - r[0], r[3] - r[2]],
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn five_shapes_keep_their_original_arms_ports_and_feed_openings() {
        for (shape, width, end, count, ground) in [
            ("quarter_wave_monopole", 0.6, [31., 0.], 2, false),
            ("meandered_monopole", 0.5, [15., 6.], 11, false),
            ("inverted_f", 0.8, [22., 8.], 8, true),
            ("meandered_inverted_f", 0.5, [13.3, 4.9], 15, true),
            ("folded_dipole", 0.9, [1.2, 0.], 6, false),
        ] {
            let g = geometry(&format!("2.4ghz_{shape}")).unwrap();
            assert_eq!(g.width, width);
            assert_eq!(g.route.len(), count);
            assert_eq!(*g.route.last().unwrap(), end);
            assert_eq!(g.ground, ground);
            assert_eq!(
                g.footprint().pads.len(),
                if g.secondary.is_some() { 2 } else { 1 }
            );
            assert!(g
                .keepouts()
                .iter()
                .all(|(at, size)| !super::super::place::inside(
                    *at,
                    &[[0., 0.], [10., 0.], [10., 10.], [0., 10.]]
                ) || size[0] > 0.));
        }
    }
}
