use opencv::imgproc::{self, TM_CCOEFF_NORMED, TM_CCORR_NORMED};
use opencv::prelude::*;

use opencv::{
    Result,
    core::{self, Mat, Point, Size},
};

type BoundingBox = (Point, Point);
type MatchResult = (BoundingBox, f64); // (bounding box, confidence score)

/// Detect multiple instances of a template in an image, handling different scales
///
/// # Arguments
/// * `needle` - Template image to search for
/// * `haystack` - Image to search in
/// * `num_matches` - Number of players/matches to find
/// * `min_scale` - Minimum scale factor to try (e.g., 0.8)
/// * `max_scale` - Maximum scale factor to try (e.g., 1.2)
/// * `scale_steps` - Number of scale steps to try between min and max
/// * `threshold` - Minimum confidence score to consider a match valid (0.0 to 1.0)
pub fn detect_needle_in_haystack(
    needle: &Mat,
    haystack: &Mat,
    num_matches: usize,
    min_scale: f64,
    max_scale: f64,
    scale_steps: usize,
    threshold: f64,
) -> Result<Vec<MatchResult>> {
    let mut matches: Vec<MatchResult> = Vec::new();
    let scale_step = (max_scale - min_scale) / (scale_steps as f64);

    // Try different scales
    for step in 0..=scale_steps {
        let scale = min_scale + (step as f64 * scale_step);
        let scaled_size = Size::new(
            (needle.cols() as f64 * scale) as i32,
            (needle.rows() as f64 * scale) as i32,
        );

        // Resize template to current scale
        let mut scaled_needle = Mat::default();
        imgproc::resize(
            needle,
            &mut scaled_needle,
            scaled_size,
            0.0,
            0.0,
            imgproc::INTER_LINEAR,
        )?;

        // Perform template matching
        let mut result = Mat::default();
        match opencv::imgproc::match_template(
            haystack,
            &scaled_needle,
            &mut result,
            TM_CCORR_NORMED,
            &core::no_array(),
        ) {
            Ok(_) => {}
            Err(e) => {
                log::info!("Failed to match template: {e}");
                continue;
            }
        };

        // Find matches above threshold
        for _ in 0..num_matches {
            let mut min_val = 0.0;
            let mut max_val = 0.0;
            let mut min_loc = Point::default();
            let mut max_loc = Point::default();

            core::min_max_loc(
                &result,
                Some(&mut min_val),
                Some(&mut max_val),
                Some(&mut min_loc),
                Some(&mut max_loc),
                &core::no_array(),
            )?;

            // If match is good enough, add it to results
            if max_val >= threshold {
                let top_left = max_loc;
                let bottom_right = Point::new(
                    top_left.x + scaled_needle.cols(),
                    top_left.y + scaled_needle.rows(),
                );
                matches.push(((top_left, bottom_right), max_val));

                // Zero out the region around the match to prevent duplicate detections
                let x1 = (max_loc.x - scaled_needle.cols() / 4).max(0);
                let y1 = (max_loc.y - scaled_needle.rows() / 4).max(0);
                let x2 = (x1 + scaled_needle.cols() + scaled_needle.cols() / 2).min(result.cols());
                let y2 = (y1 + scaled_needle.rows() + scaled_needle.rows() / 2).min(result.rows());

                if x2 > x1 && y2 > y1 {
                    let rect = core::Rect::new(x1, y1, x2 - x1, y2 - y1);
                    imgproc::rectangle(
                        &mut result,
                        rect,
                        core::Scalar::all(0.0),
                        -1, // Fill the rectangle
                        imgproc::LINE_8,
                        0,
                    )?;
                }

                if matches.len() == num_matches {
                    // Early exit
                    break;
                }
            }
        }
    }

    // Sort matches by confidence score in descending order
    matches.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Take top num_players matches
    matches.truncate(num_matches);

    Ok(matches)
}
