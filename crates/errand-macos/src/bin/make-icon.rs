//! Draws Errand's app icon to a PNG: `make-icon <output.png> [pixels]`, 1024 pixels unless given. The build
//! draws every size of the icon set with it rather than scaling one down.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("make-icon draws with AppKit, so it runs on macOS only");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(output) = arguments.first() else {
        eprintln!("usage: make-icon <output.png> [pixels]");
        std::process::exit(2);
    };
    let pixels = match arguments.get(1).map(|pixels| pixels.parse::<isize>()) {
        None => 1024,
        Some(Ok(pixels)) if pixels > 0 => pixels,
        Some(_) => {
            eprintln!("make-icon: the size is a whole number of pixels");
            std::process::exit(2);
        }
    };
    std::process::exit(if icon::write(output, pixels) { 0 } else { 1 });
}

#[cfg(target_os = "macos")]
mod icon {
    use errand_macos::shell::kit::{inset, rect};
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSAffineTransformNSAppKitAdditions, NSBezierPath, NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace, NSGraphicsContext,
    };
    use objc2_foundation::{NSAffineTransform, NSDictionary, NSPoint, NSRect};

    fn srgb(red: f64, green: f64, blue: f64) -> Retained<NSColor> {
        NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, 1.0)
    }

    fn rounded(frame: NSRect, radius: f64) -> Retained<NSBezierPath> {
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, radius, radius)
    }

    /// Finished cards collected in a tray, on a 1024 canvas. At 32 pixels and below only one card is left and
    /// the lines thicken, because two thin outlines blur into one.
    fn draw(detailed: bool) {
        let blue = srgb(0.06, 0.33, 0.83);
        let tile = rounded(rect(66.0, 66.0, 892.0, 892.0), 195.0);
        srgb(0.91, 0.94, 0.97).setFill();
        tile.fill();
        tile.setLineWidth(if detailed { 6.0 } else { 40.0 });
        tile.addClip();
        srgb(0.56, 0.61, 0.69).setStroke();
        tile.stroke();

        let line = if detailed { 32.0 } else { 64.0 };
        let cards = if detailed {
            vec![rect(265.0, 560.0, 490.0, 212.0), rect(232.0, 330.0, 558.0, 299.0)]
        } else {
            vec![rect(232.0, 330.0, 558.0, 400.0)]
        };
        for card in cards {
            // Outer edges stay where the concept put them whatever the line width.
            let shape = rounded(inset(card, line / 2.0, line / 2.0), 62.0 - line / 2.0);
            shape.setLineWidth(line);
            NSColor::whiteColor().setFill();
            shape.fill();
            blue.setStroke();
            shape.stroke();
        }

        // The tray hides the bottom of the front card; its rim dips between the two walls.
        let point = NSPoint::new;
        let tray = NSBezierPath::bezierPath();
        tray.moveToPoint(point(290.0, 224.0));
        tray.lineToPoint(point(733.0, 224.0));
        tray.curveToPoint_controlPoint1_controlPoint2(point(843.0, 334.0), point(794.0, 224.0), point(843.0, 273.0));
        tray.lineToPoint(point(843.0, 436.0));
        tray.curveToPoint_controlPoint1_controlPoint2(point(798.0, 481.0), point(843.0, 461.0), point(823.0, 481.0));
        tray.lineToPoint(point(760.0, 481.0));
        tray.curveToPoint_controlPoint1_controlPoint2(point(660.0, 374.0), point(712.0, 481.0), point(712.0, 374.0));
        tray.lineToPoint(point(363.0, 374.0));
        tray.curveToPoint_controlPoint1_controlPoint2(point(263.0, 481.0), point(311.0, 374.0), point(311.0, 481.0));
        tray.lineToPoint(point(225.0, 481.0));
        tray.curveToPoint_controlPoint1_controlPoint2(point(180.0, 436.0), point(200.0, 481.0), point(180.0, 461.0));
        tray.lineToPoint(point(180.0, 334.0));
        tray.curveToPoint_controlPoint1_controlPoint2(point(290.0, 224.0), point(180.0, 273.0), point(229.0, 224.0));
        tray.closePath();
        blue.setFill();
        tray.fill();
    }

    pub fn write(output: &str, pixels: isize) -> bool {
        let Some(bitmap) = (unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                pixels,
                pixels,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }) else {
            return false;
        };
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap).as_deref());
        let scale = NSAffineTransform::transform();
        scale.scaleBy(pixels as f64 / 1024.0);
        scale.concat();
        draw(pixels > 32);
        NSGraphicsContext::restoreGraphicsState_class();
        let png = unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) };
        png.is_some_and(|png| std::fs::write(output, png.to_vec()).is_ok())
    }
}
