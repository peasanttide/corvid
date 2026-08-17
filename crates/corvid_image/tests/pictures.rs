//! An image is a size, a format and a buffer, and the codecs it can come from.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_color::Rgba8;
use corvid_image::{
    Channels, Codec, ColorSpace, DecodeError, Image, ImageError, PixelFormat, decode, extent,
};

#[test]
fn a_buffer_of_the_wrong_length_is_refused() {
    let three_texels = vec![0u8; 3 * 4];
    assert_eq!(
        Image::new(extent(2, 2), PixelFormat::SRGBA8, three_texels.clone()),
        Err(ImageError::Length {
            extent: extent(2, 2),
            format: PixelFormat::SRGBA8,
            wanted: 16,
            given: 12,
        })
    );
    assert_eq!(
        Image::new(extent(0, 4), PixelFormat::SRGBA8, Vec::new()),
        Err(ImageError::Empty(extent(0, 4)))
    );
    assert!(Image::new(extent(3, 1), PixelFormat::SRGBA8, three_texels).is_ok());
}

#[test]
fn a_texel_is_read_by_its_channels() {
    let rows = vec![
        0x11, 0x22, 0x33, //
        0x44, 0x55, 0x66, //
        0x77, 0x88, 0x99, //
        0xaa, 0xbb, 0xcc,
    ];
    let picture = Image::new(extent(2, 2), PixelFormat::SRGB8, rows).expect("a two by two");
    assert_eq!(picture.texel(0, 0), Some(&[0x11, 0x22, 0x33][..]));
    assert_eq!(picture.texel(1, 1), Some(&[0xaa, 0xbb, 0xcc][..]));
    assert_eq!(picture.texel(2, 0), None);
    assert_eq!(picture.srgba8(1, 0), Some(Rgba8::rgb(0x44, 0x55, 0x66)));

    // A linear picture has no sRGB colour to answer with, and says so rather
    // than handing back an `Rgba8` that means something else.
    let mask = Image::new(extent(2, 1), PixelFormat::R8, vec![0x10, 0x20]).expect("a mask");
    assert_eq!(mask.srgba8(0, 0), None);
    assert_eq!(mask.texel(1, 0), Some(&[0x20][..]));
}

/// The pyramid this crate describes and does not build.
#[test]
fn a_mip_chain_halves_and_bottoms_out_at_one() {
    assert_eq!(extent(1, 1).mip_levels(), 1);
    assert_eq!(extent(256, 256).mip_levels(), 9);
    assert_eq!(extent(1024, 256).mip_levels(), 11);
    // Not a power of two: the longest side decides, and the floor is what every
    // graphics API allocates.
    assert_eq!(extent(3000, 5000).mip_levels(), 13);
    assert_eq!(extent(3000, 5000).mip(3), extent(375, 625));
    assert_eq!(extent(1024, 256).mip(9), extent(2, 1));
    assert_eq!(extent(1024, 256).mip(40), extent(1, 1));
    assert_eq!(extent(0, 8).mip_levels(), 0);
}

#[test]
fn a_format_weighs_its_channels() {
    assert_eq!(PixelFormat::R8.bytes_per_texel(), 1);
    assert_eq!(PixelFormat::SRGB8.bytes_per_texel(), 3);
    assert_eq!(PixelFormat::SRGBA8.bytes_per_texel(), 4);
    assert_eq!(Channels::from_count(3), Some(Channels::Rgb));
    assert_eq!(Channels::from_count(0), None);
    assert_eq!(Channels::from_count(5), None);
    assert_eq!(PixelFormat::SRGBA8.color_space, ColorSpace::Srgb);
    assert_eq!(PixelFormat::RGBA8.color_space, ColorSpace::Linear);
}

/// Recognising a format and being able to decode it are two different
/// questions, and the second one has a permanent answer for JPEG 2000.
#[test]
fn a_format_this_crate_cannot_decode_is_still_recognised() {
    let codestream = b"\xff\x4f\xff\x51 and then a codestream";
    assert_eq!(Codec::sniff(codestream), Some(Codec::Jpeg2000));
    assert!(!Codec::Jpeg2000.is_decodable());
    assert_eq!(
        decode(codestream),
        Err(DecodeError::NoDecoder(Codec::Jpeg2000))
    );

    let boxed = b"\x00\x00\x00\x0cjP  \r\n\x87\nftypjp2 ";
    assert_eq!(Codec::sniff(boxed), Some(Codec::Jpeg2000));

    assert_eq!(Codec::sniff(b"GIF89a"), None);
    assert_eq!(Codec::sniff(b""), None);
    assert_eq!(decode(b"GIF89a"), Err(DecodeError::Unrecognised));
}

/// What a build without a codec answers. This is the honest version of the
/// feature being off: the format is named, so a caller can say which file it
/// cannot read.
#[cfg(not(feature = "png"))]
#[test]
fn a_build_without_the_png_feature_says_so() {
    assert!(!Codec::Png.is_decodable());
    assert_eq!(
        decode(b"\x89PNG\r\n\x1a\nrubbish"),
        Err(DecodeError::NoDecoder(Codec::Png))
    );
}

#[cfg(feature = "png")]
#[test]
fn a_png_round_trips_through_the_encoder_beside_it() {
    // Encoded here rather than checked in, because a golden PNG would freeze
    // the *encoder's* choices as well as the decoder's and this test is about
    // one of them.
    let texels: Vec<u8> = (0..4u8 * 4).collect();
    let mut file = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut file, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("a header");
        writer.write_image_data(&texels).expect("four texels");
    }

    assert_eq!(Codec::sniff(&file), Some(Codec::Png));
    assert!(Codec::Png.is_decodable());
    let picture = decode(&file).expect("a picture");
    assert_eq!(picture.extent(), extent(2, 2));
    assert_eq!(picture.format(), PixelFormat::SRGBA8);
    assert_eq!(picture.texels(), texels);

    // And a truncated one is an error rather than a picture of noise.
    let mut broken = file.clone();
    broken.truncate(file.len() / 2);
    assert!(matches!(
        decode(&broken),
        Err(DecodeError::Malformed {
            codec: Codec::Png,
            ..
        })
    ));
}

#[cfg(feature = "jpeg")]
#[test]
fn a_jpeg_that_is_only_a_signature_is_malformed() {
    assert!(Codec::Jpeg.is_decodable());
    assert!(matches!(
        decode(b"\xff\xd8\xff\xe0\x00\x10JFIF\x00 and nothing else"),
        Err(DecodeError::Malformed {
            codec: Codec::Jpeg,
            ..
        })
    ));
}
