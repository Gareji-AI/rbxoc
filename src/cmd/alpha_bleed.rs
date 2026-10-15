//! Bleed RGB from opaque neighbors into fully transparent pixels (Asphalt parity).

use image::{DynamicImage, GenericImage, GenericImageView, Rgba};
use std::collections::VecDeque;

const DIRECTIONS: &[(i32, i32)] = &[
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

struct Mask2 {
    width: u32,
    data: Vec<bool>,
}

impl Mask2 {
    fn new(w: u32, h: u32) -> Self {
        Self {
            width: w,
            data: vec![false; (w * h) as usize],
        }
    }

    fn get(&self, x: u32, y: u32) -> bool {
        self.data[(x + y * self.width) as usize]
    }

    fn set(&mut self, x: u32, y: u32) {
        self.data[(x + y * self.width) as usize] = true;
    }
}

pub fn alpha_bleed(img: &mut DynamicImage) {
    let (w, h) = img.dimensions();
    let mut can_be_sampled = Mask2::new(w, h);
    let mut visited = Mask2::new(w, h);
    let mut to_visit = VecDeque::new();

    let adjacent = |x: u32, y: u32| {
        DIRECTIONS.iter().filter_map(move |(dx, dy)| {
            let xs = x as i32 + dx;
            let ys = y as i32 + dy;
            if xs < 0 || ys < 0 || xs >= w as i32 || ys >= h as i32 {
                return None;
            }
            Some((xs as u32, ys as u32))
        })
    };

    for y in 0..h {
        for x in 0..w {
            let pixel = img.get_pixel(x, y);
            if pixel[3] != 0 {
                can_be_sampled.set(x, y);
                visited.set(x, y);
                continue;
            }
            let borders_opaque = adjacent(x, y).any(|(xs, ys)| img.get_pixel(xs, ys)[3] != 0);
            if borders_opaque {
                visited.set(x, y);
                to_visit.push_back((x, y));
            }
        }
    }

    loop {
        let queue_length = to_visit.len();
        if queue_length == 0 {
            break;
        }
        let mut mutated = Vec::with_capacity(queue_length);
        for _ in 0..queue_length {
            let Some((x, y)) = to_visit.pop_front() else {
                break;
            };
            let mut sum = (0u16, 0u16, 0u16);
            let mut contributing = 0u16;
            for (xs, ys) in adjacent(x, y) {
                if can_be_sampled.get(xs, ys) {
                    let source = img.get_pixel(xs, ys);
                    contributing += 1;
                    sum.0 += source[0] as u16;
                    sum.1 += source[1] as u16;
                    sum.2 += source[2] as u16;
                } else if !visited.get(xs, ys) {
                    visited.set(xs, ys);
                    to_visit.push_back((xs, ys));
                }
            }
            let d = contributing.max(1);
            img.put_pixel(
                x,
                y,
                Rgba([(sum.0 / d) as u8, (sum.1 / d) as u8, (sum.2 / d) as u8, 0]),
            );
            mutated.push((x, y));
        }
        for (x, y) in mutated {
            can_be_sampled.set(x, y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn bleeds_transparent_edge_from_opaque_neighbor() {
        let mut img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 0])));
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        alpha_bleed(&mut img);
        assert_eq!(img.get_pixel(1, 0)[0], 255);
        assert_eq!(img.get_pixel(1, 0)[3], 0);
    }
}
