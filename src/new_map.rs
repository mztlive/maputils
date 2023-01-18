use std::{
    fs::{self},
    io::{Cursor, Seek, SeekFrom},
};

use image::{Rgba, RgbaImage};

use crate::buffer_utils;

/// 地图文件头
pub struct MapHeader {
    pub flag: u32,
    pub width: u32,
    pub height: u32,
    pub map_index_list: Vec<u32>,
    pub rows: u32,
    pub cols: u32,
    pub index_size: u32,
}

/// 地图单元数据（小图片）
pub struct Unit {
    pub unit_flag: String,
    pub size: u32,
    pub unit_data: Vec<u8>,
}

/// 遮罩数据
pub struct Mask {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    size: u32,
    data: Vec<u8>,
}

/// 地图数据
pub struct Map {
    pub map_header: MapHeader,
    pub units: Vec<Unit>,
    pub masks: Vec<Mask>,
}

/// 读取文件头
fn read_header(file: &mut Cursor<Vec<u8>>) -> anyhow::Result<MapHeader> {
    let flag_bytes = buffer_utils::read_bytes(file, 4)?;
    let flag_str = String::from_utf8(flag_bytes.clone())?;

    if flag_str != "0.1M" {
        return Err(anyhow::anyhow!("Invalid map file"));
    }

    let flag = u32::from_le_bytes([flag_bytes[0], flag_bytes[1], flag_bytes[2], flag_bytes[3]]);
    let width = buffer_utils::read_u32(file)?;
    let height = buffer_utils::read_u32(file)?;

    let rows = ((height as f32) / 240.00).ceil() as u32;
    let cols = ((width as f32) / 320.00).ceil() as u32;
    let index_size = rows * cols;
    let index_bytes = buffer_utils::read_bytes(file, (index_size * 4) as usize)?;
    let map_index_list = index_bytes
        .chunks(4)
        .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
        .collect();

    Ok(MapHeader {
        flag,
        width,
        height,
        map_index_list,
        rows,
        cols,
        index_size,
    })
}

/// 读取遮罩数据 (遮罩的图片是被压缩的，需要解压)
/// 这个方法应该是有问题的
fn read_mask(file: &mut Cursor<Vec<u8>>) -> anyhow::Result<Vec<Mask>> {
    let unknown = buffer_utils::read_u32(file)?;
    let mask_num = buffer_utils::read_u32(file)?;
    let mask_data = buffer_utils::read_bytes(file, (mask_num * 4) as usize)?;
    let masks_offsets = mask_data
        .chunks(4)
        .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
        .collect::<Vec<u32>>();

    let mut masks = Vec::new();
    for offset in masks_offsets {
        file.seek(SeekFrom::Start(offset as u64))?;

        let x = buffer_utils::read_u32(file)?;
        let y = buffer_utils::read_u32(file)?;
        let width = buffer_utils::read_u32(file)?;
        let height = buffer_utils::read_u32(file)?;
        let size = buffer_utils::read_u32(file)?;
        let data = buffer_utils::read_bytes(file, (size) as usize)?;

        let aiginw = ((width >> 2) + if (width % 4 != 0) { 1 } else { 0 }) << 2;
        let mask_index = (aiginw * height) >> 2;

        let mut decompressed_data = vec![0; mask_index as usize];
        let out = decompressed_data.as_mut_slice();
        let out = rust_lzo::LZOContext::decompress_to_slice(data.as_slice(), out);

        if out.1 != rust_lzo::LZOError::OK {
            return Err(anyhow::anyhow!("Decompress mask data failed"));
        }

        let mut mask_data: Vec<i64> = vec![0; (width * height) as usize];
        let mut desc: usize = 0;
        for k in 0..height {
            for i in 0..width {
                let index = (k * aiginw + i) << 1 as usize;
                let mask = out.0[(index >> 3) as usize];
                let mask = mask >> (index % 8);
                if mask & 3 == 3 {
                    mask_data[desc] = 0xF0;
                }

                desc += 1;
            }
        }

        let mut image = RgbaImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let index = y * width + x;
            let color = mask_data[index as usize];
            let r = ((color >> 11) & 0x1F) << 3;
            let g = ((color >> 5) & 0x3F) << 2;
            let b = (color & 0x1F) << 3;
            let a = ((color >> 16) & 0x1F) << 3;
            *pixel = Rgba([r as u8, g as u8, b as u8, a as u8]);
        }

        image.save(format!("masks/{}.png", offset)).unwrap();

        let mask = Mask {
            x,
            y,
            width,
            height,
            size,
            data: out.0.to_vec(),
        };

        masks.push(mask);
    }

    Ok(masks)
}

/// 读取图片并转码
fn read_jpeg(map_file: &mut Cursor<Vec<u8>>, unit: &mut Unit) -> anyhow::Result<()> {
    unit.unit_data = buffer_utils::read_bytes(map_file, unit.size as usize)?;

    // 这段代码的逻辑是参考 https://www.jianshu.com/p/7faf26c9648a 实现的
    let mut is_ffda = false;
    for index in 0..unit.unit_data.len() {
        if !is_ffda {
            if unit.unit_data[index] == 0xFF && unit.unit_data[index + 1] == 0xDA {
                unit.unit_data[index + 3] = 0x0C;

                // +13位的意思是说： index当前是ff的位置， ff后面总共还有12位数据，其中 DA 1位， 长度2位， 9位数据
                unit.unit_data.insert(index + 13, 0x00);
                unit.unit_data.insert(index + 14, 0x3F);
                unit.unit_data.insert(index + 15, 0x00);
                is_ffda = true;
            }
        } else {
            if unit.unit_data[index] == 0xFF {
                if unit.unit_data[index + 1] == 0xD9 {
                    break;
                }
                unit.unit_data.insert(index + 1, 0x00);
            }
        }
    }

    // 这段代码是参考一个C#版本实现的,和上面的有些类似，
    // 但是逻辑上是不一样的， 不过上面的代码也能实现同样的功能，还不知道为什么，先注释测试再看吧

    // let mut is_filled = false;
    // let jpeg_buffer = vec![];
    // let mut jpeg_buffer = Cursor::new(jpeg_buffer);
    // jpeg_buffer.write(&unit.unit_data[0..2])?;

    // let mut p: usize = 4;
    // let mut start = 4;

    // while p < (unit.size - 2) as usize {
    //     if !is_filled && unit.unit_data[p] == 0xFF {
    //         p = p + 1;
    //         if unit.unit_data[p] == 0xDA {
    //             is_filled = true;
    //             unit.unit_data[p + 2] = 0x0C;
    //             jpeg_buffer.write(&unit.unit_data[start as usize..p + 10])?;
    //             let write_data: [u8; 3] = [0x00, 0x3F, 0x00];
    //             jpeg_buffer.write(&write_data)?;
    //             start = p + 10;
    //             p = p + 9;
    //         }
    //     }

    //     if is_filled && unit.unit_data[p] == 0xFF {
    //         jpeg_buffer.write(&unit.unit_data[start as usize..p + 1])?;
    //         let empty = [0x00; 1];
    //         jpeg_buffer.write(&empty)?;
    //         start = p + 1;
    //     }

    //     p = p + 1;
    // }

    // jpeg_buffer.write(&unit.unit_data[start as usize..(unit.size) as usize])?;
    // unit.unit_data = jpeg_buffer.into_inner().to_vec();
    Ok(())
}

/// 读取每一个单元的数据
fn read_unit(map_header: &MapHeader, map_file: &mut Cursor<Vec<u8>>) -> anyhow::Result<Vec<Unit>> {
    let mut units: Vec<Unit> = vec![];

    for index in map_header.map_index_list.iter() {
        let mut unit = Unit {
            unit_flag: "".to_string(),
            size: 0,
            unit_data: vec![],
        };

        map_file.seek(SeekFrom::Start(*index as u64))?;

        // 这两个数据未知，不知道用来干什么的
        let unkonwn = buffer_utils::read_u32(map_file)?;
        let unkonwn_data = buffer_utils::read_bytes(map_file, (4 * unkonwn) as usize)?;

        let unit_head = buffer_utils::read_bytes(map_file, 8)?;
        unit.unit_flag = String::from_utf8(unit_head[0..4].to_vec())?;
        unit.size = u32::from_le_bytes(unit_head[4..8].try_into()?);
        if unit.unit_flag == "GEPJ" {
            // 这种类型的的图片要进行解码
            read_jpeg(map_file, &mut unit)?;
            units.push(unit);

        // 这里是参考了SeeMap这个软件的源码才知道有一个 2GPJ 的类型
        } else if unit.unit_flag == "2GPJ" {
            // 这种类型的的图片是完整的jpeg
            unit.unit_data = buffer_utils::read_bytes(map_file, unit.size as usize)?;
            units.push(unit);
        }
    }
    Ok(units)
}

/// 读取地图文件到内存中
fn load_mapfile(filename: &str) -> anyhow::Result<Cursor<Vec<u8>>> {
    let mut file = fs::read(filename)?;
    let cursor = Cursor::new(file);
    Ok(cursor)
}

pub fn decode(filename: &str) -> anyhow::Result<Map> {
    let mut bytes = load_mapfile(filename)?;
    let header = read_header(&mut bytes)?;
    let masks = read_mask(&mut bytes)?;
    let uints = read_unit(&header, &mut bytes)?;

    let map = Map {
        map_header: header,
        masks,
        units: uints,
    };
    Ok(map)
}

#[cfg(test)]
mod tests {
    use image::{imageops, RgbaImage};

    use super::*;

    #[test]
    fn it_works() {
        let filename = "1003.map";
        let mut bytes = load_mapfile(filename).unwrap();
        let header = read_header(&mut bytes).unwrap();
        let masks = read_mask(&mut bytes).unwrap();
        let uints = read_unit(&header, &mut bytes).unwrap();

        let mut bk = RgbaImage::new(header.width, header.height);
        for i in 0..header.rows {
            for j in 0..header.cols {
                let index = i * header.cols + j;
                let unit = &uints[index as usize];
                let unit_image = image::load_from_memory(&unit.unit_data).unwrap();
                imageops::overlay(&mut bk, &unit_image, (j * 320) as i64, (i * 240) as i64);
            }
        }
        bk.save(format!("{}.jpg", 1003)).unwrap();
    }
}
