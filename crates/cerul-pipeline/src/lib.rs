pub mod chunking;
pub mod ffmpeg;
pub mod mlx_sidecar;
pub mod run;
mod stages;
pub mod whisper;

pub fn crate_ready() -> bool {
    true
}
