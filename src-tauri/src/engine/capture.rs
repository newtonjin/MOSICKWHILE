//! Screen capture backend used to feed the optical-flow analyzer.

use super::flow::{FlowSample, OpticalFlowAnalyzer};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct CaptureReading {
    pub flow: FlowSample,
    pub latency_ms: f32,
    pub ok: bool,
    pub status: String,
}

pub struct ScreenMotionCapture {
    #[cfg(windows)]
    manager: Option<dxgi_capture_rs::DXGIManager>,
    analyzer: OpticalFlowAnalyzer,
    last_fps_at: Instant,
    frames: u32,
    analyze_fps: f32,
    last_error: String,
}

impl Default for ScreenMotionCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenMotionCapture {
    pub fn new() -> Self {
        let mut cap = Self {
            #[cfg(windows)]
            manager: None,
            analyzer: OpticalFlowAnalyzer::default(),
            last_fps_at: Instant::now(),
            frames: 0,
            analyze_fps: 0.0,
            last_error: String::new(),
        };
        cap.reinit();
        cap
    }

    pub fn analyze_fps(&self) -> f32 {
        self.analyze_fps
    }

    pub fn reinit(&mut self) {
        self.analyzer.reset();
        self.last_error.clear();
        #[cfg(windows)]
        {
            match dxgi_capture_rs::DXGIManager::new(30) {
                Ok(mut mgr) => {
                    // Capture primary output; geometry follows the active desktop.
                    let _ = mgr.set_capture_source_index(0);
                    self.manager = Some(mgr);
                    self.last_error.clear();
                }
                Err(e) => {
                    self.manager = None;
                    self.last_error = format!("DXGI init failed: {e:?}");
                }
            }
        }
        #[cfg(not(windows))]
        {
            self.last_error = "Screen capture is Windows-only".into();
        }
    }

    pub fn poll(&mut self) -> CaptureReading {
        #[cfg(windows)]
        {
            self.poll_windows()
        }
        #[cfg(not(windows))]
        {
            CaptureReading {
                flow: FlowSample::default(),
                latency_ms: 0.0,
                ok: false,
                status: self.last_error.clone(),
            }
        }
    }

    #[cfg(windows)]
    fn poll_windows(&mut self) -> CaptureReading {
        if self.manager.is_none() {
            self.reinit();
        }

        let Some(manager) = self.manager.as_mut() else {
            return CaptureReading {
                flow: FlowSample::default(),
                latency_ms: 0.0,
                ok: false,
                status: self.last_error.clone(),
            };
        };

        let started = Instant::now();
        match manager.capture_frame() {
            Ok((pixels, (width, height))) => {
                // Flatten BGRA8 structs into bytes for the analyzer.
                let mut bgra = Vec::with_capacity(pixels.len() * 4);
                for p in &pixels {
                    bgra.push(p.b);
                    bgra.push(p.g);
                    bgra.push(p.r);
                    bgra.push(p.a);
                }

                let flow = self
                    .analyzer
                    .push_bgra(&bgra, width, height)
                    .unwrap_or_default();

                self.frames += 1;
                let elapsed = self.last_fps_at.elapsed().as_secs_f32();
                if elapsed >= 1.0 {
                    self.analyze_fps = self.frames as f32 / elapsed;
                    self.frames = 0;
                    self.last_fps_at = Instant::now();
                }

                CaptureReading {
                    flow,
                    latency_ms: started.elapsed().as_secs_f32() * 1000.0,
                    ok: true,
                    status: "capturing".into(),
                }
            }
            Err(dxgi_capture_rs::CaptureError::Timeout) => CaptureReading {
                flow: FlowSample::default(),
                latency_ms: started.elapsed().as_secs_f32() * 1000.0,
                ok: true,
                status: "waiting_frame".into(),
            },
            Err(e) => {
                self.last_error = format!("{e:?}");
                // Force reinit next tick on hard failures.
                self.manager = None;
                CaptureReading {
                    flow: FlowSample::default(),
                    latency_ms: started.elapsed().as_secs_f32() * 1000.0,
                    ok: false,
                    status: self.last_error.clone(),
                }
            }
        }
    }
}
