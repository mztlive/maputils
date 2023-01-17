use std::io::Read;

pub fn read_u8<T: Read>(buffer: &mut T) -> anyhow::Result<u8> {
    let mut buf = [0; 1];
    buffer.read(&mut buf)?;
    Ok(buf[0])
}

pub fn read_u16<T: Read>(buffer: &mut T) -> anyhow::Result<u16> {
    let mut buf = [0u8; 2];
    buffer.read(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

pub fn read_u32<T: Read>(file: &mut T) -> anyhow::Result<u32> {
    let mut buffer: [u8; 4] = [0; 4];
    file.read(&mut buffer)?;
    Ok(u32::from_le_bytes(buffer))
}

pub fn read_bytes<T: Read>(buffer: &mut T, size: usize) -> anyhow::Result<Vec<u8>> {
    let mut buf = vec![0u8; size];
    buffer.read(&mut buf)?;
    Ok(buf)
}
