//! Headless check of the macOS video path: opens the QuickTime or MPEG-4
//! file named by `PICTO_GPUI_TEST_VIDEO` through the player's AVFoundation
//! backend, plays it muted and pulls frames as the window does. The frames
//! must be the bi-planar full-range buffers GPUI's `surface` draws, of the
//! item's size, carrying the BT.601 matrix the composition asks for, at
//! advancing times; a seek must move the clock.
//!
//! AVFoundation delivers its state on the main thread's run loop, which the
//! standard harness does not give a test, so this is a plain program
//! (`harness = false`) that skips without the variable:
//!
//! ```sh
//! PICTO_GPUI_TEST_VIDEO=/path/to/video.mov cargo test --test avfoundation_frames
//! ```

#[cfg(target_os = "macos")]
// The modules' own unit tests are not run here.
#[allow(dead_code, unused_imports)]
#[path = "../src/viewer/video"]
mod video {
    pub mod audio;
    pub mod avf;
    pub mod backend;
    pub mod dsp;
    pub mod ffmpeg;
    pub mod model;
}

#[cfg(target_os = "macos")]
fn main() {
    use objc2_core_foundation::{CFRunLoop, CFString, kCFRunLoopDefaultMode};
    use objc2_core_video::{
        CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
        kCVImageBufferYCbCrMatrix_ITU_R_601_4, kCVImageBufferYCbCrMatrixKey,
        kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    };
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use video::backend::{Backend, Frame};

    let Ok(path) = std::env::var("PICTO_GPUI_TEST_VIDEO") else {
        println!("avfoundation_frames: skipped (set PICTO_GPUI_TEST_VIDEO)");
        return;
    };
    assert!(objc2::MainThreadMarker::new().is_some());
    let path = PathBuf::from(path);
    // Runs the main run loop for a moment, so AVFoundation's updates land.
    let spin = |duration: Duration| unsafe {
        CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, duration.as_secs_f64(), false);
    };
    let mut player = video::avf::AvPlayer::open(&path).unwrap();
    player.set_volume(0., true);
    player.play();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut frames = Vec::new();
    while frames.len() < 12 {
        let status = player.status();
        assert!(
            Instant::now() < deadline,
            "{} frames by the deadline: {status:?}",
            frames.len()
        );
        assert!(status.error.is_none(), "{:?}", status.error);
        if let Some(Frame::Surface(buffer)) = player.take_frame() {
            let format = CVPixelBufferGetPixelFormatType(&buffer);
            assert_eq!(format, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange);
            let size = (
                CVPixelBufferGetWidth(&buffer) as u32,
                CVPixelBufferGetHeight(&buffer) as u32,
            );
            let matrix =
                unsafe { buffer.attachment(kCVImageBufferYCbCrMatrixKey, std::ptr::null_mut()) }
                    .and_then(|value| value.downcast::<CFString>().ok())
                    .map(|value| value.to_string());
            frames.push((status.position, size, status.size, matrix));
        }
        spin(Duration::from_millis(4));
    }
    let status = player.status();
    println!("{}: {status:?}", path.display());
    for frame in &frames {
        println!(
            "  position {:.3}s, buffer {:?}, item {:?}, matrix {:?}",
            frame.0, frame.1, frame.2, frame.3
        );
    }
    let first = &frames[0];
    assert!(frames.iter().all(|frame| frame.1 == first.1), "one size");
    assert_eq!(Some(first.1), status.size, "frames are the item's size");
    assert!(
        frames.windows(2).all(|pair| pair[1].0 >= pair[0].0) && frames[11].0 > first.0,
        "times advance"
    );
    let expected = unsafe { kCVImageBufferYCbCrMatrix_ITU_R_601_4 }.to_string();
    assert_eq!(
        frames.last().unwrap().3.as_deref(),
        Some(expected.as_str()),
        "the composition writes BT.601"
    );
    // A seek moves the clock there, playing or not.
    player.pause();
    player.seek(2.);
    let deadline = Instant::now() + Duration::from_secs(5);
    while (player.status().position - 2.).abs() > 0.05 {
        assert!(Instant::now() < deadline, "seek: {:?}", player.status());
        spin(Duration::from_millis(10));
    }
    // And the picture there arrives while paused.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "no picture after the seek");
        if player.take_frame().is_some() {
            break;
        }
        spin(Duration::from_millis(10));
    }
    println!(
        "avfoundation_frames: ok ({} frames, seek and paused picture)",
        frames.len()
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("avfoundation_frames: skipped (macOS only)");
}
