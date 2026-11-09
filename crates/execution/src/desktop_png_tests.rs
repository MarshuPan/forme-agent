use super::*;

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&[1, 2, 3, 255, 4, 5, 6, 255])
        .unwrap();
    writer.finish().unwrap();
    bytes
}
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = (data.len() as u32).to_be_bytes().to_vec();
    bytes.extend(kind);
    bytes.extend(data);
    bytes.extend(crc32fast::hash(&bytes[4..]).to_be_bytes());
    bytes
}
#[test]
fn decode_rejects_valid_crc_with_corrupt_image_data_and_dimensions() {
    let original = png();
    validate_desktop_png(&original, 2, 1).unwrap();
    for (width, height) in [(0, 1), (2, 0), (1, 2), (3, 1), (u32::MAX, u32::MAX)] {
        assert!(validate_desktop_png(&original, width, height).is_err());
    }
    let mut corrupt = original[..33].to_vec();
    corrupt.extend(chunk(b"IDAT", b"invalid-compressed-pixels"));
    corrupt.extend(chunk(b"IEND", b""));
    assert!(validate_desktop_png(&corrupt, 2, 1).is_err());
    let mut short = original[..33].to_vec();
    // Valid zlib encoding of zero bytes, insufficient to encode a scanline.
    short.extend(chunk(
        b"IDAT",
        &[0x78, 0x01, 0x01, 0, 0, 0xff, 0xff, 0, 0, 0, 1],
    ));
    short.extend(chunk(b"IEND", b""));
    assert!(validate_desktop_png(&short, 2, 1).is_err());
}
#[test]
fn strict_container_rejects_animation_duplicate_headers_crc_and_tail() {
    let original = png();
    for (kind, body) in [
        (*b"IHDR", original[16..29].to_vec()),
        (*b"acTL", vec![0; 8]),
        (*b"fcTL", vec![0; 26]),
        (*b"fdAT", vec![0; 4]),
        (*b"ABCD", vec![]),
        (*b"abca", vec![]),
        (*b"ab1D", vec![]),
    ] {
        let mut bytes = original[..33].to_vec();
        bytes.extend(chunk(&kind, &body));
        bytes.extend(&original[33..]);
        assert!(validate_desktop_png(&bytes, 2, 1).is_err(), "{kind:?}");
    }
    let mut valid_text = original[..33].to_vec();
    valid_text.extend(chunk(b"tEXt", b"fixture\0original"));
    valid_text.extend(&original[33..]);
    validate_desktop_png(&valid_text, 2, 1).unwrap();
    valid_text[42] ^= 1; // Bad ancillary CRC must fail even when text is ignored.
    assert!(validate_desktop_png(&valid_text, 2, 1).is_err());
    for ending in [b"tail".to_vec(), chunk(b"IEND", b""), png()] {
        let mut bytes = original.clone();
        bytes.extend(ending);
        assert!(validate_desktop_png(&bytes, 2, 1).is_err());
    }
    for length in 0..original.len() {
        assert!(validate_desktop_png(&original[..length], 2, 1).is_err());
    }
}
#[test]
fn separate_idat_sequences_and_bad_palette_are_rejected() {
    let original = png();
    let iend = original.len() - 12;
    let mut bytes = original[..iend].to_vec();
    bytes.extend(chunk(b"tEXt", b"key\0value"));
    bytes.extend(chunk(b"IDAT", b""));
    bytes.extend(&original[iend..]);
    assert!(validate_desktop_png(&bytes, 2, 1).is_err());
    for palette in [vec![], vec![1], vec![0; 771]] {
        let mut bytes = original[..33].to_vec();
        bytes.extend(chunk(b"PLTE", &palette));
        bytes.extend(&original[33..]);
        assert!(validate_desktop_png(&bytes, 2, 1).is_err());
    }
}
