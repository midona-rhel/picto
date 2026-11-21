//! A check of video playback on macOS that opens no window. It opens the
//! QuickTime or MPEG-4 file that `PICTO_GPUI_TEST_VIDEO` names with the
//! player's AVFoundation backend, plays it muted, and takes frames from the
//! player as the viewer's window does. It checks that the frames are bi-planar
//! full-range buffers, which is what GPUI's `surface` draws, that they have
//! the item's size, that they carry the BT.601 matrix that the video
//! composition asks for, and that their times advance. It also checks that a
//! seek moves the player's position.
//!
//! AVFoundation reports its state on the run loop of the main thread, and the
//! standard test harness does not run a test there. So this is a plain program
//! (`harness = false`). It skips the check when the variable is not set:
//!
//! ```sh
//! PICTO_GPUI_TEST_VIDEO=/path/to/video.mov cargo test --test avfoundation_frames
//! ```

#[cfg(target_os = "macos")]
// This program includes the player's modules from the app's sources. It does
// not run their unit tests and does not use all of their code, so the warnings
// about unused code and unused imports are turned off.
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
    // Runs the main run loop for `duration`, so that AVFoundation can deliver
    // its updates.
    let spin = |duration: Duration| unsafe {
        CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, duration.as_secs_f64(), false);
    };
    // A player that was opened and not started still gives its first frame,
    // because the player seeks to the start when it opens. A file in a
    // collection is opened this way. The frame must arrive within 10 seconds
    // and must be at the start of the video.
    for _ in 0..2 {
        let opened = Instant::now();
        let mut paused = video::avf::AvPlayer::open(&path).unwrap();
        paused.set_volume(0., true);
        let deadline = opened + Duration::from_secs(10);
        loop {
            let status = paused.status();
            assert!(
                Instant::now() < deadline,
                "no first frame while paused: {status:?}"
            );
            if paused.take_frame().is_some() {
                assert!(
                    status.position < 0.05,
                    "the first frame is the start: {status:?}"
                );
                println!(
                    "  paused: first frame at {:.3}s after {:?}",
                    status.position,
                    opened.elapsed()
                );
                break;
            }
            spin(Duration::from_millis(4));
        }
    }
    let opened = Instant::now();
    let mut player = video::avf::AvPlayer::open(&path).unwrap();
    player.set_volume(0., true);
    player.play();
    let mut first_reported = false;
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
            if !first_reported {
                first_reported = true;
                println!(
                    "  playing: first frame at {:.3}s after {:?}",
                    status.position,
                    opened.elapsed()
                );
                assert!(status.position < 0.1, "the first frame is the start");
            }
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
    // A seek moves the player's position to the target while the player is
    // paused.
    player.pause();
    player.seek(2.);
    let deadline = Instant::now() + Duration::from_secs(5);
    while (player.status().position - 2.).abs() > 0.05 {
        assert!(Instant::now() < deadline, "seek: {:?}", player.status());
        spin(Duration::from_millis(10));
    }
    // The frame at the new position arrives while the player is paused.
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
