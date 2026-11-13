//! Errand's original attachment icon. The matching editable SVG is assets/icons/attachment.svg.
use super::Element;
pub const BODY: &str = "M13 4.5H7.25A1.75 1.75 0 0 0 5.5 6.25v11.5c0 .97.78 1.75 1.75 1.75h9.5c.97 0 1.75-.78 1.75-1.75V10L13 4.5Z";
pub const FOLD: &str = "M13 4.5v3.75c0 .97.78 1.75 1.75 1.75h3.75";
pub const LINES: &str = "M8.5 13h7M8.5 16h4.5";
pub const ELEMENTS: &[Element] = &[Element::Path(BODY), Element::Path(FOLD), Element::Path(LINES)];
