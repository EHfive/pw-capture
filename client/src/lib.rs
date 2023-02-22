mod client;
mod format;
mod spa_utils;
mod stream;

pub use client::*;
pub use format::*;
pub(crate) use spa_utils::*;
pub use stream::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
