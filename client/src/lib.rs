mod client;
mod format;
mod spa_utils;
mod stream;
mod utils;

pub use client::*;
pub use format::*;
pub(crate) use spa_utils::*;
pub use stream::*;
pub(crate) use utils::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
