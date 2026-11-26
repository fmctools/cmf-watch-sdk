//! The bitmap-lv screen: one baked bitmap on the stock page, as docs/bitmap-assets.md shows it. The host test
//! builds the batch and validates it against the loader's own checker (wp2-core lvcmd), IMAGE_SRC included.
#![no_std]

use cmf_wp2_app::lv::*;
use cmf_wp2_app::stock;

cmf_wp2_app::bitmaps!();

use bitmaps::ORB;

/// The page, title and the image object.
pub fn build(b: &mut Batch) -> Result<(Obj, Obj), LvError> {
    stock::theme(b)?;
    let (page, _bar) = stock::page(b)?;
    stock::title(b, page, "Bitmap")?;
    let img = b.image(page)?;
    b.set_image(img, ORB.px, ORB.w, ORB.h)?;
    b.align(img, Align::Center, 0, 40)?;
    Ok((page, img))
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;
    use wp2_core::lvcmd::{validate, Table};

    #[test]
    fn image_batch_validates_against_the_loader() {
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        let (_page, img) = build(&mut b).unwrap();
        assert!(img != ROOT);
        // stock::theme points styles at the system font, open in slot 0 before this batch (as on the watch)
        assert_eq!(validate(b.bytes(), &Table::new(), 1 << stock::FONT), Ok(()));
    }
}
