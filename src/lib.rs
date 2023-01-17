mod buffer_utils;
mod map;

pub fn decode(filename: &str) -> anyhow::Result<map::Map> {
    map::decode(filename)
}
