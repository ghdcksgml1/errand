//! The menu bar item: one small battery per service, and the menu that opens the apps and the widget.

use super::kit::{Action, ns};
use super::marks::app_mark_image;
use block2::RcBlock;
use errand_core::view::{MenuAction, MenuEntry, StatusIconVm};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBezierPath, NSCellImagePosition, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuItem,
    NSStatusBar, NSStatusItem,
};
use objc2_foundation::{NSRect, NSSize};
use std::rc::Rc;

pub struct StatusMenu {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
    icon: Option<StatusIconVm>,
    entries: Option<Vec<MenuEntry>>,
    /// Menu items hold their targets weakly.
    actions: Vec<Retained<Action>>,
}

impl StatusMenu {
    pub fn new(mtm: MainThreadMarker) -> StatusMenu {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        if let Some(button) = item.button(mtm) {
            button.setImagePosition(NSCellImagePosition::ImageOnly);
        }
        StatusMenu { mtm, item, icon: None, entries: None, actions: Vec::new() }
    }

    pub fn render(&mut self, icon: StatusIconVm, entries: Vec<MenuEntry>, on_action: &Rc<dyn Fn(MenuAction)>) {
        if self.icon.as_ref() != Some(&icon) {
            if let Some(button) = self.item.button(self.mtm) {
                button.setImage(Some(&battery_image(&icon.levels)));
                button.setToolTip(Some(&ns(&icon.tooltip)));
                button.setAccessibilityLabel(Some(&ns(&icon.tooltip)));
            }
            self.icon = Some(icon);
        }
        if self.entries.as_ref() != Some(&entries) {
            self.actions.clear();
            let menu = self.build(&entries, on_action);
            self.item.setMenu(Some(&menu));
            // The app's own menu, shown while it is in front, offers the same.
            let main = NSMenu::new(self.mtm);
            let app_item = NSMenuItem::new(self.mtm);
            app_item.setSubmenu(Some(&self.build(&entries, on_action)));
            main.addItem(&app_item);
            NSApplication::sharedApplication(self.mtm).setMainMenu(Some(&main));
            self.entries = Some(entries);
        }
    }

    fn build(&mut self, entries: &[MenuEntry], on_action: &Rc<dyn Fn(MenuAction)>) -> Retained<NSMenu> {
        let menu = NSMenu::new(self.mtm);
        menu.setAutoenablesItems(false);
        for entry in entries {
            let MenuEntry::Item(vm) = entry else {
                menu.addItem(&NSMenuItem::separatorItem(self.mtm));
                continue;
            };
            let key = vm.key.map(String::from).unwrap_or_default();
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(self.mtm), &ns(&vm.title), vm.action.map(|_| sel!(perform:)), &ns(&key))
            };
            if let Some(action) = vm.action {
                let on_action = on_action.clone();
                let target = Action::new(self.mtm, move || on_action(action));
                unsafe { item.setTarget(Some(&target)) };
                self.actions.push(target);
            }
            item.setEnabled(vm.enabled);
            item.setState(if vm.checked { NSControlStateValueOn } else { NSControlStateValueOff });
            item.setIndentationLevel(isize::from(vm.indent));
            if let Some(tooltip) = &vm.tooltip {
                item.setToolTip(Some(&ns(tooltip)));
            }
            if let Some(provider) = vm.icon {
                item.setImage(Some(&app_mark_image(provider, 16.0)));
            }
            menu.addItem(&item);
        }
        menu
    }
}

/// One small battery per service, filled to what remains; unknown ones stay empty. A template image, so the
/// menu bar tints it.
pub fn battery_image(levels: &[Option<i64>]) -> Retained<NSImage> {
    let levels = levels.to_vec();
    let handler = RcBlock::new(move |_: NSRect| -> Bool {
        NSColor::blackColor().set();
        let (height, gap) = (5.0, 3.0);
        let count = levels.len() as f64;
        let top = (16.0 - count * height - (count - 1.0) * gap) / 2.0;
        for (index, level) in levels.iter().enumerate() {
            let body = super::kit::rect(1.5, top + index as f64 * (height + gap), 17.0, height);
            let outline = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(body, 1.8, 1.8);
            outline.setLineWidth(1.0);
            outline.stroke();
            NSBezierPath::fillRect(super::kit::rect(body.origin.x + body.size.width + 1.0, body.origin.y + height / 2.0 - 1.2, 1.5, 2.4));
            let Some(level) = level else { continue };
            let mut fill = super::kit::inset(body, 1.5, 1.5);
            fill.size.width *= (*level).clamp(0, 100) as f64 / 100.0;
            if fill.size.width > 0.0 {
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(fill, 0.8, 0.8).fill();
            }
        }
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(22.0, 16.0), true, &handler);
    image.setTemplate(true);
    image
}
