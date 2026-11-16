//! ICC profiles: moxcms's own, encoded, and lookup-table profiles of any
//! size, written here.

use moxcms::ColorProfile;

/// moxcms's sRGB profile, encoded.
#[must_use]
pub fn srgb() -> Vec<u8> {
    ColorProfile::new_srgb().encode().unwrap()
}

/// moxcms's Display P3 profile, encoded.
#[must_use]
pub fn display_p3() -> Vec<u8> {
    ColorProfile::new_display_p3().encode().unwrap()
}

/// moxcms's Adobe RGB (1998) profile, encoded.
#[must_use]
pub fn adobe_rgb() -> Vec<u8> {
    ColorProfile::new_adobe_rgb().encode().unwrap()
}

/// moxcms's grey profile with a gamma of 2.2, encoded.
#[must_use]
pub fn grey() -> Vec<u8> {
    ColorProfile::new_gray_with_gamma(2.2).encode().unwrap()
}

/// The colour space of a [`lut`] profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    /// RGB, a display profile.
    Rgb,
    /// CMYK, a printer profile.
    Cmyk,
}

/// A profile whose device-to-connection transform (`A2B0`) is a 16-bit
/// lookup table (`mft2`) of `grid` points a channel, with values that do not
/// repeat (so nothing about it compresses away), and identity curves.
#[must_use]
pub fn lut(space: Space, grid: u8) -> Vec<u8> {
    let inputs: u8 = match space {
        Space::Rgb => 3,
        Space::Cmyk => 4,
    };
    let mut table = b"mft2".to_vec();
    table.extend([0; 4]);
    table.extend([inputs, 3, grid, 0]);
    for row in 0..3 {
        for column in 0..3 {
            table.extend(fixed(if row == column { 1.0 } else { 0.0 }));
        }
    }
    // Two-entry curves, 0 to 65535, in and out.
    table.extend(2_u16.to_be_bytes());
    table.extend(2_u16.to_be_bytes());
    let curves = |n: u8| {
        (0..n)
            .flat_map(|_| [0_u16, u16::MAX])
            .flat_map(u16::to_be_bytes)
    };
    table.extend(curves(inputs));
    let points = u32::from(grid).pow(u32::from(inputs));
    for i in 0..points {
        let value = u16::try_from(i.wrapping_mul(2_654_435_761) & 0xffff).unwrap();
        for _ in 0..3 {
            table.extend(value.to_be_bytes());
        }
    }
    table.extend(curves(3));
    let white = [
        b"XYZ ".as_slice(),
        &[0; 4],
        &fixed(0.9642),
        &fixed(1.0),
        &fixed(0.8249),
    ]
    .concat();
    let (class, signature) = match space {
        Space::Rgb => (*b"mntr", *b"RGB "),
        Space::Cmyk => (*b"prtr", *b"CMYK"),
    };
    profile(class, signature, &[(*b"wtpt", white), (*b"A2B0", table)])
}

/// A matrix RGB profile (Display P3's primaries) whose three tone curves
/// are tables of `entries` entries each (a gamma of about 2.2).
#[must_use]
pub fn with_curves(entries: u32) -> Vec<u8> {
    let xyz = |x: f64, y: f64, z: f64| {
        [b"XYZ ".as_slice(), &[0; 4], &fixed(x), &fixed(y), &fixed(z)].concat()
    };
    let mut curve = b"curv".to_vec();
    curve.extend([0; 4]);
    curve.extend(entries.to_be_bytes());
    for i in 0..entries {
        let t = f64::from(i) / f64::from(entries.max(2) - 1);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "0 to 65535"
        )]
        let value = (t.powf(2.2) * 65_535.0).round() as u16;
        curve.extend(value.to_be_bytes());
    }
    profile(
        *b"mntr",
        *b"RGB ",
        &[
            (*b"wtpt", xyz(0.9642, 1.0, 0.8249)),
            (*b"rXYZ", xyz(0.5151, 0.2412, -0.0011)),
            (*b"gXYZ", xyz(0.2920, 0.6922, 0.0419)),
            (*b"bXYZ", xyz(0.1571, 0.0666, 0.7841)),
            (*b"rTRC", curve.clone()),
            (*b"gTRC", curve.clone()),
            (*b"bTRC", curve),
        ],
    )
}

/// A profile of `class`, colour space `space` and connection space XYZ,
/// holding `tags`.
fn profile(class: [u8; 4], space: [u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_len = 4 + 12 * tags.len();
    let mut entries = Vec::new();
    let mut data = Vec::new();
    for (signature, body) in tags {
        while !(128 + table_len + data.len()).is_multiple_of(4) {
            data.push(0);
        }
        entries.extend_from_slice(signature);
        entries.extend(
            u32::try_from(128 + table_len + data.len())
                .unwrap()
                .to_be_bytes(),
        );
        entries.extend(u32::try_from(body.len()).unwrap().to_be_bytes());
        data.extend_from_slice(body);
    }
    let size = 128 + table_len + data.len();
    let mut header = Vec::with_capacity(128);
    header.extend(u32::try_from(size).unwrap().to_be_bytes());
    header.extend(b"none");
    header.extend(0x0210_0000_u32.to_be_bytes());
    header.extend(class);
    header.extend(space);
    header.extend(b"XYZ ");
    header.extend([0; 12]);
    header.extend(b"acsp");
    // Platform, flags, manufacturer, model, attributes, intent.
    header.extend([0; 28]);
    header.extend(fixed(0.9642));
    header.extend(fixed(1.0));
    header.extend(fixed(0.8249));
    header.resize(128, 0);
    let mut out = header;
    out.extend(u32::try_from(tags.len()).unwrap().to_be_bytes());
    out.extend(entries);
    out.extend(data);
    out
}

/// `value` as an ICC s15Fixed16Number.
fn fixed(value: f64) -> [u8; 4] {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the values written are small"
    )]
    let raw = (value * 65_536.0).round() as i32;
    raw.to_be_bytes()
}
