use anyhow::Result;
use opencv::{
    core::{MatTraitConst, Rect, Scalar, Vector},
    imgcodecs::{self, imwrite},
    imgproc::{self, LINE_8},
};
use wordle_timer_bot::detection::detect_needle_in_haystack;

#[test]
fn test_end_game_detection() -> Result<()> {
    let haystack = imgcodecs::imread("./data/preview.png", imgcodecs::IMREAD_COLOR_RGB)?;
    let needle = imgcodecs::imread("./data/solved.png", imgcodecs::IMREAD_COLOR_RGB)?;

    let boxes = detect_needle_in_haystack(&needle, &haystack, 2, 0.6, 1.4, 100, 0.9)?;
    let mut display_image = haystack.clone();

    for (b, confidence) in boxes.iter() {
        println!("Confidence: {confidence}");
        let top_left = b.0;
        imgproc::rectangle(
            &mut display_image,
            Rect::new(top_left.x, top_left.y, needle.cols(), needle.rows()),
            Scalar::new(0.0, 255.0, 0.0, 0.0),
            2,
            LINE_8,
            0,
        )?;
    }

    imwrite("test.png", &display_image, &Vector::new())?;

    Ok(())
}

fn test_avatar_detection() -> Result<()> {
    Ok(())
}
