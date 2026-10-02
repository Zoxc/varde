use super::*;

/// A `width` by `height` image whose pixels count up.
fn image(width: u32, height: u32) -> Image {
    let rgba = (0..width * height * 4).map(|i| i as u8).collect();
    Image::new(width, height, rgba).unwrap()
}

#[test]
fn images_are_checked_as_made() {
    assert!(Image::new(2, 3, vec![0; 24]).is_some());
    assert_eq!(Image::new(2, 3, vec![0; 23]), None);
    assert_eq!(Image::new(0, 3, Vec::new()), None);
    assert_eq!(
        Image::new(MAX_SIDE + 1, 1, vec![0; (MAX_SIDE as usize + 1) * 4]),
        None
    );
    assert_eq!(Image::new(u32::MAX, u32::MAX, Vec::new()), None);
}

#[test]
fn images_are_checked_from_the_wire() {
    let sent = image(3, 2);
    let bytes = postcard::to_stdvec(&sent).unwrap();
    assert_eq!(postcard::from_bytes::<Image>(&bytes).unwrap(), sent);
    // A row short.
    #[derive(Serialize)]
    struct Raw<'a> {
        width: u32,
        height: u32,
        rgba: &'a [u8],
    }
    let raw = Raw {
        width: 3,
        height: 3,
        rgba: sent.rgba(),
    };
    let bytes = postcard::to_stdvec(&raw).unwrap();
    assert!(postcard::from_bytes::<Image>(&bytes).is_err());
}

#[test]
fn a_thumbnail_comes_back_as_it_went() {
    let sent = image(17, 5);
    let preview = encode(&sent).unwrap();
    assert!(preview.is("image/png"));
    assert_eq!(decode(&preview), Some(sent));
}

/// `pixels` of `color` and 8 bit `depth` as a PNG preview.
fn png(width: u32, height: u32, color: png::ColorType, pixels: &[u8]) -> Preview {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(pixels).unwrap();
    writer.finish().unwrap();
    Preview::new(MEDIA_TYPE, bytes).unwrap()
}

#[test]
fn other_colour_types_are_made_rgba() {
    let rgb = png(2, 1, png::ColorType::Rgb, &[1, 2, 3, 4, 5, 6]);
    assert_eq!(decode(&rgb).unwrap().rgba(), [1, 2, 3, 255, 4, 5, 6, 255]);
    let grey = png(2, 1, png::ColorType::Grayscale, &[7, 8]);
    assert_eq!(decode(&grey).unwrap().rgba(), [7, 7, 7, 255, 8, 8, 8, 255]);
    let grey_alpha = png(1, 1, png::ColorType::GrayscaleAlpha, &[9, 10]);
    assert_eq!(decode(&grey_alpha).unwrap().rgba(), [9, 9, 9, 10]);
}

#[test]
fn what_isnt_a_thumbnail_decodes_to_nothing() {
    let sent = encode(&image(4, 4)).unwrap();
    let jpeg = Preview::new("image/jpeg", sent.data().to_vec()).unwrap();
    assert_eq!(decode(&jpeg), None);
    let cut = Preview::new(MEDIA_TYPE, sent.data()[..sent.data().len() / 2].to_vec()).unwrap();
    assert_eq!(decode(&cut), None);
    let junk = Preview::new(MEDIA_TYPE, vec![0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
    assert_eq!(decode(&junk), None);
    // Too wide, refused by its header.
    let wide = png(
        MAX_SIDE + 1,
        1,
        png::ColorType::Grayscale,
        &vec![0; MAX_SIDE as usize + 1],
    );
    assert_eq!(decode(&wide), None);
}
