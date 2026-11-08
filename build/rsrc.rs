//! 将多尺寸 ICO 和通用控件 manifest 转为 COFF 资源节，无需外部资源编译器。

/// 只声明现代通用控件；DPI 感知仍由程序入口设置。
const MANIFEST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

pub struct IconFrame<'a> {
    // ICO 条目的前 12 字节也适用于组图标条目，保留尺寸及颜色信息。
    entry: [u8; 12],
    data: &'a [u8],
}

pub fn parse_ico(ico: &[u8]) -> Result<Vec<IconFrame<'_>>, &'static str> {
    if ico.len() < 6 || read_u16(ico, 0) != 0 || read_u16(ico, 2) != 1 {
        return Err("不是有效的 ICO 文件头");
    }
    let count = read_u16(ico, 4) as usize;
    // 为组图标和 manifest 保留两条重定位，避免触发 COFF 扩展重定位计数格式。
    if count == 0 || count >= u16::MAX as usize - 2 || ico.len() < 6 + count * 16 {
        return Err("ICO 图像数量或目录长度无效");
    }
    let mut frames = Vec::with_capacity(count);
    for index in 0..count {
        let entry = 6 + index * 16;
        let size = read_u32(ico, entry + 8) as usize;
        let offset = read_u32(ico, entry + 12) as usize;
        let end = offset.checked_add(size).ok_or("ICO 图像范围溢出")?;
        if ico[entry + 3] != 0 || size == 0 || offset < 6 + count * 16 || end > ico.len() {
            return Err("ICO 图像数据范围或保留字段无效");
        }
        frames.push(IconFrame {
            entry: ico[entry..entry + 12].try_into().unwrap(),
            // PNG 和 DIB 均原样嵌入，避免损坏高分辨率或透明度数据。
            data: &ico[offset..end],
        });
    }
    Ok(frames)
}

pub fn build_rsrc_coff(frames: &[IconFrame<'_>], machine: u16, reloc_type: u16) -> Vec<u8> {
    assert!(!frames.is_empty() && frames.len() < u16::MAX as usize - 2);
    let count = frames.len();
    let ids: Vec<u32> = (1..=count as u32).collect();
    let mut section = Vec::new();

    // 资源树依次为类型、名称、语言；全部使用整数 ID，目录项按 ID 升序。
    let root = directory(&mut section, &[3, 14, 24]);
    let icons = directory(&mut section, &ids);
    let group = directory(&mut section, &[1]);
    let manifest = directory(&mut section, &[1]);
    subdirectory(&mut section, root, 2, manifest);
    subdirectory(&mut section, root, 0, icons);
    subdirectory(&mut section, root, 1, group);
    let mut languages = Vec::new();
    for index in 0..count {
        let language = directory(&mut section, &[0]);
        subdirectory(&mut section, icons, index, language);
        languages.push(language);
    }
    let language = directory(&mut section, &[0]);
    subdirectory(&mut section, group, 0, language);
    languages.push(language);
    let language = directory(&mut section, &[0]);
    subdirectory(&mut section, manifest, 0, language);
    languages.push(language);

    let mut entries = Vec::new();
    for language in languages {
        let entry = section.len();
        section.resize(entry + 16, 0);
        write_u32(&mut section, language + 20, entry as u32);
        entries.push(entry);
    }

    let mut group_data = vec![0, 0, 1, 0];
    group_data.extend_from_slice(&(count as u16).to_le_bytes());
    for (index, frame) in frames.iter().enumerate() {
        group_data.extend_from_slice(&frame.entry);
        // 组图标引用 RT_ICON 的 16 位资源 ID，而不是 ICO 文件偏移。
        group_data.extend_from_slice(&((index + 1) as u16).to_le_bytes());
    }
    for (entry, data) in entries.iter().zip(
        frames
            .iter()
            .map(|frame| frame.data)
            .chain([group_data.as_slice(), MANIFEST]),
    ) {
        align_eight(&mut section);
        let offset = u32::try_from(section.len()).expect("图标资源过大");
        let size = u32::try_from(data.len()).expect("图标帧过大");
        write_u32(&mut section, *entry, offset);
        write_u32(&mut section, *entry + 4, size);
        section.extend_from_slice(data);
    }
    align_eight(&mut section);

    let reloc_offset = 60 + section.len();
    let symbol_offset = reloc_offset + entries.len() * 10;
    let mut object = vec![0; 60];
    // COFF 文件头（20 字节）及唯一的节头（40 字节）。
    write_u16(&mut object, 0, machine);
    write_u16(&mut object, 2, 1);
    write_u32(&mut object, 8, symbol_offset as u32);
    write_u32(&mut object, 12, 1);
    object[20..28].copy_from_slice(b".rsrc\0\0\0");
    write_u32(&mut object, 36, section.len() as u32);
    write_u32(&mut object, 40, 60);
    write_u32(&mut object, 44, reloc_offset as u32);
    write_u16(&mut object, 52, entries.len() as u16);
    write_u32(&mut object, 56, 0x40000040);
    object.extend_from_slice(&section);

    // 每个 OffsetToData 都通过节符号的 ADDR32NB 重定位转换为最终 RVA。
    for entry in entries {
        object.extend_from_slice(&(entry as u32).to_le_bytes());
        object.extend_from_slice(&0_u32.to_le_bytes());
        object.extend_from_slice(&reloc_type.to_le_bytes());
    }
    let mut symbol = [0; 18];
    symbol[..8].copy_from_slice(b".rsrc\0\0\0");
    write_u16(&mut symbol, 12, 1);
    symbol[16] = 3; // IMAGE_SYM_CLASS_STATIC，指向第 1 节。
    object.extend_from_slice(&symbol);
    object.extend_from_slice(&4_u32.to_le_bytes());
    object
}

fn directory(section: &mut Vec<u8>, ids: &[u32]) -> usize {
    let offset = section.len();
    section.resize(offset + 16 + ids.len() * 8, 0);
    write_u16(section, offset + 14, ids.len() as u16);
    for (index, id) in ids.iter().enumerate() {
        write_u32(section, offset + 16 + index * 8, *id);
    }
    offset
}

fn subdirectory(section: &mut [u8], parent: usize, index: usize, child: usize) {
    write_u32(section, parent + 20 + index * 8, child as u32 | 0x80000000);
}

fn align_eight(bytes: &mut Vec<u8>) {
    bytes.resize((bytes.len() + 7) & !7, 0);
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICO: &[u8] = include_bytes!("../assets/liftly.ico");

    #[test]
    fn parses_all_real_icon_frames_without_changing_data() {
        let frames = parse_ico(ICO).unwrap();
        assert_eq!(frames.len(), 8);
        for (index, size) in [16, 20, 24, 32, 40, 48, 64, 0].iter().enumerate() {
            assert_eq!(frames[index].entry[0], *size);
            assert_eq!(frames[index].entry[1], *size);
            let entry = 6 + index * 16;
            let offset = read_u32(ICO, entry + 12) as usize;
            let length = read_u32(ICO, entry + 8) as usize;
            assert_eq!(frames[index].data, &ICO[offset..offset + length]);
        }
        assert!(frames[7].data.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn rejects_invalid_headers_and_frame_ranges() {
        for length in [0, 5, 6, 133, ICO.len() - 1] {
            assert!(parse_ico(&ICO[..length]).is_err());
        }
        for (offset, value) in [(0, 1), (2, 2), (4, 0), (9, 1)] {
            let mut invalid = ICO.to_vec();
            invalid[offset] = value;
            assert!(parse_ico(&invalid).is_err());
        }
        for offset in [0, 6, u32::MAX] {
            let mut invalid = ICO.to_vec();
            write_u32(&mut invalid, 18, offset);
            assert!(parse_ico(&invalid).is_err());
        }
    }

    #[test]
    fn coff_headers_and_relocations_match_each_architecture() {
        let frames = parse_ico(ICO).unwrap();
        for (machine, relocation) in [(0x8664, 3), (0xaa64, 2), (0x014c, 7)] {
            let object = build_rsrc_coff(&frames, machine, relocation);
            assert_eq!(read_u16(&object, 0), machine);
            assert_eq!(read_u16(&object, 2), 1);
            assert_eq!(read_u32(&object, 12), 1);
            assert_eq!(&object[20..28], b".rsrc\0\0\0");
            assert_eq!(read_u32(&object, 56), 0x40000040);
            assert_eq!(read_u16(&object, 52), 10);
            let relocations = read_u32(&object, 44) as usize;
            let symbols = read_u32(&object, 8) as usize;
            assert_eq!(symbols, relocations + 10 * 10);
            for index in 0..10 {
                let relocation_offset = relocations + index * 10;
                let entry = 60 + read_u32(&object, relocation_offset) as usize;
                assert_eq!(read_u32(&object, relocation_offset + 4), 0);
                assert_eq!(read_u16(&object, relocation_offset + 8), relocation);
                assert_eq!(read_u32(&object, entry) % 8, 0);
                assert_eq!(read_u32(&object, entry + 8), 0);
                assert_eq!(read_u32(&object, entry + 12), 0);
            }
            assert_eq!(&object[symbols..symbols + 8], b".rsrc\0\0\0");
            assert_eq!(read_u16(&object, symbols + 12), 1);
            assert_eq!(object[symbols + 16], 3);
            assert_eq!(&object[symbols + 18..], &4_u32.to_le_bytes());
        }
    }

    // 独立遍历生成的目录，验证目录标志、排序和语言叶子。
    fn children(section: &[u8], directory: usize) -> Vec<(u32, u32)> {
        assert_eq!(read_u16(section, directory + 12), 0);
        (0..read_u16(section, directory + 14) as usize)
            .map(|index| {
                let entry = directory + 16 + index * 8;
                (read_u32(section, entry), read_u32(section, entry + 4))
            })
            .collect()
    }

    #[test]
    fn resource_tree_and_group_ids_reference_all_frames() {
        let frames = parse_ico(ICO).unwrap();
        let object = build_rsrc_coff(&frames, 0x8664, 3);
        let section = &object[60..60 + read_u32(&object, 36) as usize];
        let types = children(section, 0);
        assert_eq!(
            types.iter().map(|item| item.0).collect::<Vec<_>>(),
            [3, 14, 24]
        );
        for (kind, names) in types {
            assert_ne!(names & 0x80000000, 0);
            let names = children(section, (names & 0x7fffffff) as usize);
            assert_eq!(names.len(), if kind == 3 { 8 } else { 1 });
            for (index, (id, language)) in names.into_iter().enumerate() {
                assert_eq!(id, (index + 1) as u32);
                assert_ne!(language & 0x80000000, 0);
                let languages = children(section, (language & 0x7fffffff) as usize);
                assert_eq!(languages.len(), 1);
                assert_eq!(languages[0].0, 0);
                let entry = languages[0].1;
                assert_eq!(entry & 0x80000000, 0);
                let start = read_u32(section, entry as usize) as usize;
                let size = read_u32(section, entry as usize + 4) as usize;
                let data = &section[start..start + size];
                if kind == 3 {
                    assert_eq!(data, frames[index].data);
                } else if kind == 14 {
                    assert_eq!(read_u16(data, 0), 0);
                    assert_eq!(read_u16(data, 2), 1);
                    assert_eq!(read_u16(data, 4), 8);
                    assert_eq!(data.len(), 6 + 8 * 14);
                    for (index, frame) in frames.iter().enumerate() {
                        let offset = 6 + index * 14;
                        assert_eq!(&data[offset..offset + 12], &frame.entry);
                        assert_eq!(read_u16(data, offset + 12), (index + 1) as u16);
                    }
                } else {
                    assert_eq!(data, MANIFEST);
                }
            }
        }
    }

    #[test]
    fn manifest_is_embedded_verbatim_with_neutral_language_and_id_one() {
        let frames = parse_ico(ICO).unwrap();
        let object = build_rsrc_coff(&frames, 0x8664, 3);
        let section = &object[60..60 + read_u32(&object, 36) as usize];
        let types = children(section, 0);
        let names = children(section, (types[2].1 & 0x7fffffff) as usize);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].0, 1);
        let languages = children(section, (names[0].1 & 0x7fffffff) as usize);
        assert_eq!(languages.len(), 1);
        assert_eq!(languages[0].0, 0);
        let entry = languages[0].1 as usize;
        let start = read_u32(section, entry) as usize;
        let size = read_u32(section, entry + 4) as usize;
        assert_eq!(&section[start..start + size], MANIFEST);
    }
}
