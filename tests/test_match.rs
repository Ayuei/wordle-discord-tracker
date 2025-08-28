use anyhow::Result;
use opencv::{
    core::{MatTraitConst, Rect, Scalar, Vector},
    imgcodecs::{self, imwrite},
    imgproc::{self, LINE_8},
};
use serial_test::serial;
use wordle_timer_bot::{Player, detection::detect_needle_in_haystack, verify_player_completion};

#[serial]
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

#[serial]
#[tokio::test]
async fn test_avatar_detection_all_match() -> Result<()> {
    let mut alice = Player::new(1, "alice".to_string(), "".to_string());
    let mut bob = Player::new(2, "bob".to_string(), "".to_string());

    alice.downloaded_fp = Some("./converted/tnf_214332607326978048.png".to_string());
    bob.downloaded_fp = Some("./converted/probablybob_265081770758635522.png".to_string());

    verify_player_completion(&mut alice, "./data/daily_end.png".to_string()).await?;
    verify_player_completion(&mut bob, "./data/daily_end.png".to_string()).await?;

    for player in vec![alice, bob] {
        assert!(player.completed);
    }

    Ok(())
}

#[serial]
#[tokio::test]
async fn test_avatar_detection_both_match() -> Result<()> {
    let mut alice = Player::new(1, "alice".to_string(), "".to_string());
    let mut bob = Player::new(2, "bob".to_string(), "".to_string());

    alice.downloaded_fp = Some("./converted/tnf_214332607326978048.png".to_string());
    bob.downloaded_fp = Some("./converted/probablybob_265081770758635522.png".to_string());

    verify_player_completion(&mut alice, "./data/two_player.webp".to_string()).await?;
    verify_player_completion(&mut bob, "./data/two_player.webp".to_string()).await?;

    for player in vec![alice, bob] {
        assert!(player.completed);
    }

    Ok(())
}

#[serial]
#[tokio::test]
async fn test_avatar_detection_one_match() -> Result<()> {
    let mut alice = Player::new(1, "alice".to_string(), "".to_string());
    let mut bob = Player::new(2, "bob".to_string(), "".to_string());

    alice.downloaded_fp = Some("./converted/tnf_214332607326978048.png".to_string());
    bob.downloaded_fp = Some("./converted/probablybob_265081770758635522.png".to_string());

    verify_player_completion(&mut alice, "./data/preview.png".to_string()).await?;
    verify_player_completion(&mut bob, "./data/preview.png".to_string()).await?;

    assert!(alice.completed);
    assert!(bob.completed == false);

    Ok(())
}

#[serial]
#[test]
fn draw_rectangle_test() -> Result<()> {
    let haystack = imgcodecs::imread("./data/two_player.webp", imgcodecs::IMREAD_COLOR_RGB)?;
    let needle = imgcodecs::imread(
        "./converted/probablybob_265081770758635522.png",
        imgcodecs::IMREAD_COLOR_RGB,
    )?;

    let boxes = detect_needle_in_haystack(&needle, &haystack, 1, 0.1, 1.0, 100, 0.84)?;
    let mut display_image = haystack.clone();

    for (b, confidence) in boxes.iter() {
        println!("Confidence: {confidence}");
        let top_left = b.0;
        let bottom_right = b.1;

        println!("Top left: {:?}", b.0);
        println!("Bottom right: {:?}", b.1);

        let width = bottom_right.x - top_left.x;
        let height = bottom_right.y - top_left.y;

        println!("{height} x {width}");

        imgproc::rectangle(
            &mut display_image,
            Rect::new(top_left.x, top_left.y, width, height),
            Scalar::new(0.0, 255.0, 0.0, 0.0),
            2,
            LINE_8,
            0,
        )?;
    }

    imwrite("test.png", &display_image, &Vector::new())?;
    imwrite("needle.png", &needle, &Vector::new())?;

    Ok(())
}
