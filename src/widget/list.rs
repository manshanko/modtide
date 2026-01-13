use std::fmt::Write;
use std::path::PathBuf;

use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::mod_engine::ModEngine;
use crate::mod_engine::ModState;
use super::Control;
use super::WidgetConfig;
use super::button;
use super::button::ButtonWidget;
use super::Event;
use super::EventKind;
use super::KeyKind;

pub struct ModListWidget {
    background: ID2D1Bitmap,
    brush: ID2D1SolidColorBrush,
    text_format: IDWriteTextFormat,

    mods_path: PathBuf,
    lorder: ModEngine,
    builtins: Vec<&'static str>,
    using_aml: bool,

    scroll: i32,
    item_height: i32,
    clicked_mod: Option<usize>,
    mouse_drag_y: Option<i32>,
    mouse_hover_y: Option<i32>,
    selected: Vec<usize>,
    selected_pivot: usize,
    select_defer: Option<bool>,
    dropdown_defer: bool,
}

impl ModListWidget {
    pub const WIDTH: u32 = 770;
    pub const HEIGHT: u32 = 560;

    const MODTIDE_HEADER_PREFIX: &str = "-- Modified by modtide";

    const TEXT_PADDING: u32 = 12;
    const MARGIN_X: u32 = 35;
    const MARGIN_Y: u32 = 32;
    const MARGIN_RIGHT: u32 = ButtonWidget::MARGIN_RIGHT;
    const MARGIN_TOP: u32 = button::EXIT_X_OFFSET + button::EXIT_Y_OFFSET + button::EXIT_HEIGHT - 10;
    const WIDTH_INNER: u32 = 700;
    const HEIGHT_INNER: u32 = 496;

    const ITEM_HEIGHT: u32 = 22;

    const FALLBACK_BACKGROUND: [f32; 4] = [0.0, 0.0, 0.0, 0.8];
    const FALLBACK_BORDER: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    const MOD_BUILTIN_GOLD: [f32; 4] = [
        220.0 / 255.0,
        190.0 / 255.0,
        60.0 / 255.0,
        1.0,
    ];
    const MOD_ENABLED_BLUE: [f32; 4] = [
        71.0 / 255.0,
        196.0 / 255.0,
        208.0 / 255.0,
        1.0,
    ];
    const MOD_DISABLED_GRAY: [f32; 4] = [
        102.0 / 255.0,
        102.0 / 255.0,
        102.0 / 255.0,
        1.0,
    ];
    const MOD_MISSING_ENTRY_ORANGE: [f32; 4] = [0.8, 0.5, 0.0, 1.0];
    const MOD_NOT_INSTALLED_RED: [f32; 4] = [0.6, 0.2, 0.2, 1.0];
    const MOD_HIGHLIGHT: [f32; 4] = [0.2, 0.2, 0.2, 0.5];
    const MOD_ENTRY_LENGTH: f32 = 320.0;

    pub fn new(
        mods_path: impl Into<PathBuf>,
        background: ID2D1Bitmap,
        brush: ID2D1SolidColorBrush,
        text_format: IDWriteTextFormat,
    ) -> Self {
        Self {
            background,
            brush,
            text_format,

            mods_path: mods_path.into(),
            lorder: ModEngine::new(),
            builtins: Vec::new(),
            using_aml: false,

            scroll: 0,
            item_height: Self::ITEM_HEIGHT as i32,
            clicked_mod: None,
            mouse_drag_y: None,
            mouse_hover_y: None,
            selected: Vec::new(),
            selected_pivot: 0,
            select_defer: None,
            dropdown_defer: false,
        }
    }

    pub fn fallback(
        context: &mut super::DrawScope,
        brush: &ID2D1SolidColorBrush,
    ) {
        let rect = [
            (Self::MARGIN_X - 2) as f32,
            (Self::MARGIN_Y - 2) as f32,
            (Self::MARGIN_X + 2 + Self::WIDTH_INNER) as f32,
            (Self::MARGIN_Y + 2 + Self::HEIGHT_INNER) as f32,
        ];
        let radius = 8.0;

        unsafe {
            brush.SetColor(Self::FALLBACK_BACKGROUND.as_ptr() as *const _);
        }
        context.fill_rounded_rect(
            brush,
            rect,
            radius,
        );

        unsafe {
            brush.SetColor(Self::FALLBACK_BORDER.as_ptr() as *const _);
        }
        context.draw_rounded_rect(
            brush,
            rect,
            radius,
            2.0,
        );
    }

    pub fn mount(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.using_aml = false;

        self.builtins.clear();
        self.mods_path.push("base/mod_manager.lua");
        if let Ok(data) = std::fs::read_to_string(&self.mods_path) {
            self.builtins.push("Darktide Mod Loader");
            if data.contains("AML") {
                self.using_aml = true;
                self.builtins.push("AML");
            }
        }
        self.mods_path.pop();
        self.mods_path.pop();

        self.mods_path.push("dmf/dmf.mod");
        if self.mods_path.exists() {
            self.builtins.push("Darktide Mod Framework");
        }
        self.mods_path.pop();
        self.mods_path.pop();

        let _owner;
        let load_order = if self.using_aml {
            ""
        } else {
            _owner = match std::fs::read_to_string(self.mods_path.join("mod_load_order.txt")) {
                Ok(s) => s,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(err) => return Err(err.into()),
            };
            if let Some((first, rest)) = _owner.split_once('\n') {
                if first.starts_with(Self::MODTIDE_HEADER_PREFIX) {
                    rest
                } else {
                    &_owner
                }
            } else {
                &_owner
            }
        };

        let paths = ModEngine::scan(&self.mods_path)?;
        self.lorder.load(load_order, &paths)?;

        Ok(())
    }

    fn update_mod_lorder(&self) {
        let mut out = String::new();
        out.push_str(Self::MODTIDE_HEADER_PREFIX);
        let res;
        unsafe {
            let time = windows::Win32::System::SystemInformation::GetLocalTime();
            res = write!(&mut out, " on {}-{:02}-{:02}T{:02}:{:02}:{:02}",
                time.wYear, time.wMonth, time.wDay,
                time.wHour, time.wMinute, time.wSecond,
            );
        }
        out.push('\n');

        if res.is_ok() && self.lorder.generate(&mut out).is_ok() {
            std::fs::write(self.mods_path.join("mod_load_order.txt"), out).unwrap();
        }
    }

    fn toggle_mod(&mut self, entry: usize, enable: Option<bool>) -> bool {
        let Some(m) = self.lorder.mods.get_mut(entry) else {
            return false;
        };

        let new_state = match (enable, m.state.clone()) {
            (Some(true), ModState::Enabled) => ModState::Enabled,
            (Some(false), ModState::Disabled | ModState::MissingEntry)
                => ModState::Disabled,

            (_, ModState::Enabled) => ModState::Disabled,
            (_, ModState::Disabled | ModState::MissingEntry)
                => ModState::Enabled,

            _ => m.state.clone(),
        };

        if new_state != m.state {
            m.state = new_state;
            true
        } else {
            false
        }
    }

    fn get_entry(&self, y: i32) -> Entry {
        let top = Self::MARGIN_Y as i32;
        let offset = y - top;
        if offset < 0 || offset > Self::HEIGHT_INNER as i32 {
            Entry::None
        } else {
            let offset = self.scroll + offset;
            let entry = (offset / self.item_height) as usize;
            if let Some(_builtin) = self.builtins.get(entry) {
                Entry::Builtin(entry)
            } else {
                Entry::Mod(entry - self.builtins.len())
            }
        }
    }

    fn get_slot(&self, y: i32) -> (usize, u32) {
        let mut min_offset = self.builtins.len() as i32 * self.item_height;
        let mut max_offset = (self.builtins.len() + self.lorder.mods.len()) as i32 * self.item_height;

        if self.scroll > min_offset {
            min_offset = self.scroll;
            let diff = min_offset % self.item_height;
            if diff != 0 {
                min_offset += self.item_height - diff;
            }
        }

        if self.scroll + (Self::HEIGHT_INNER as i32) < max_offset {
            max_offset = self.scroll + Self::HEIGHT_INNER as i32;
            max_offset -= max_offset % self.item_height;
        }

        let mut start = self.scroll;
        let diff = start % self.item_height;
        if diff != 0 {
            start += self.item_height - diff;
        }
        start = start.max(min_offset);

        let mut end = self.scroll + Self::HEIGHT_INNER as i32;
        end -= end % self.item_height;
        end = end.min(max_offset);

        let mut offset = self.scroll + y - Self::MARGIN_Y as i32;
        offset += self.item_height / 2;
        offset -= offset % self.item_height;

        let slot = offset.min(end).max(start);
        let mut entry = slot / self.item_height;
        entry = entry.saturating_sub(self.builtins.len() as i32);
        let entry = entry as usize;

        let offset = slot - self.scroll + Self::MARGIN_Y as i32;
        let offset = offset
            .min((Self::MARGIN_Y + Self::HEIGHT_INNER) as i32)
            .max(0);

        assert!(slot >= 0);
        assert!(slot % self.item_height == 0);
        assert!(entry < self.lorder.mods.len());
        (entry, offset as u32)
    }

    fn move_selected(
        &mut self,
        to: usize,
    ) -> bool {
        self.selected.sort();
        let mods = &mut self.lorder.mods;

        debug_assert!(!self.selected.is_empty());
        debug_assert!(!mods.is_empty());
        debug_assert!(to < mods.len());

        let to = to.min(mods.len().saturating_sub(1));
        let intersect = match self.selected.binary_search(&to) {
            Ok(i) => i,
            Err(i) => i,
        };
        let to = to - intersect;

        let mut tmp = Vec::new();
        self.selected.reverse();
        for i in self.selected.drain(..) {
            tmp.push(mods.remove(i));
        }
        tmp.reverse();

        let len = tmp.len();
        mods.splice(to..to, tmp);

        for i in 0..len {
            self.selected.push(to + i);
        }

        // we don't check if redraw is necessary yet
        true
    }

    fn draw_mod(
        &self,
        context: &mut super::DrawScope,
        text: &str,
        color: [f32; 4],
        o: u32,
        hovered: bool,
        selected: bool,
    ) {
        let left = Self::MARGIN_X;
        let top = Self::MARGIN_Y;
        let item_height = self.item_height as u32;

        if hovered {
            unsafe {
                self.brush.SetColor(Self::MOD_HIGHLIGHT.as_ptr() as *const _);
            }

            let mid = (top + o + self.item_height as u32 / 2) as f32;
            let from = [
                left as f32 + 6.0,
                mid,
            ];
            let to = [
                left as f32 + Self::MOD_ENTRY_LENGTH,
                mid,
            ];
            context.draw_line(from, to, &self.brush, 18.0);
        }

        unsafe {
            self.brush.SetColor(color.as_ptr() as *const _);
        }

        let rect = [
            (left + Self::TEXT_PADDING) as f32,
            (top + o) as f32,
            (left + Self::WIDTH_INNER) as f32,
            (top + o + item_height) as f32,
        ];
        context.draw_text(
            text.as_ref(),
            &self.text_format,
            &self.brush,
            &rect,
        );

        if selected {
            unsafe {
                self.brush.SetColor(color.as_ptr() as *const _);
            }

            let mid = (top + o + self.item_height as u32 / 2) as f32;
            let from = [
                left as f32 + 8.0,
                mid,
            ];
            let to = [
                left as f32 + 4.0,
                mid,
            ];
            context.draw_line(from, to, &self.brush, 22.0);
        }
    }

    fn update_mouse_drag(
        &mut self,
        drag_y: Option<i32>,
    ) -> bool {
        if drag_y != self.mouse_drag_y {
            if drag_y.is_none() || self.mouse_drag_y.is_none() {
                return true;
            } else if let Some(drag1) = drag_y
                && let Some(drag2) = self.mouse_drag_y
                && let (_, draw1) = self.get_slot(drag1)
                && let (_, draw2) = self.get_slot(drag2)
                && draw1 != draw2
            {
                return true;
            }
        }
        false
    }

    fn update_mouse_hover(
        &mut self,
        hover_y: Option<i32>,
    ) -> bool {
        if hover_y != self.mouse_hover_y {
            if hover_y.is_none() || self.mouse_hover_y.is_none() {
                return true;
            } else if let Some(hover1) = hover_y
                && let Some(hover2) = self.mouse_hover_y
                && self.get_entry(hover1) != self.get_entry(hover2)
            {
                return true;
            }
        }
        false
    }
}

#[derive(PartialEq)]
enum Entry {
    Mod(usize),
    Builtin(usize),
    None,
}

impl super::Widget for ModListWidget {
    fn config(&self) -> WidgetConfig {
        WidgetConfig {
            listen_double_click: true,
        }
    }

    fn rect(&self, width: u32, _height: u32) -> [u32; 4] {
        let size = unsafe { self.background.GetPixelSize() };
        [
            width + Self::MARGIN_X - Self::MARGIN_RIGHT - size.width,
            Self::MARGIN_TOP,
            width + Self::MARGIN_X - Self::MARGIN_RIGHT,
            Self::MARGIN_TOP + size.height,
        ]
    }

    fn handle_event(
        &mut self,
        control: &mut super::ControlScope,
        event: Event,
    ) {
        let x = event.x;
        let y = event.y;

        let left = Self::MARGIN_X as i32;
        let top = Self::MARGIN_Y as i32;
        let right = left + Self::WIDTH_INNER as i32;
        let bottom = top + Self::HEIGHT_INNER as i32;

        let is_inside = x >= left && x < right
            && y >= top && y < bottom;

        match event.kind {
            EventKind::MouseLeave => {
                let hover = self.mouse_hover_y;
                self.mouse_hover_y = None;
                if self.update_mouse_hover(hover) {
                    control.redraw();
                }
            }

            EventKind::MouseMove => {
                let drag = self.mouse_drag_y;
                let hover = self.mouse_hover_y;
                if let Some(drag) = &mut self.mouse_drag_y {
                    *drag = y;
                } else if let Some(clicked) = self.clicked_mod
                    && let entry = self.get_entry(y)
                    && (entry != Entry::Mod(clicked) || entry == Entry::None)
                {
                    self.mouse_hover_y = None;
                    self.mouse_drag_y = Some(y);
                } else if is_inside {
                    self.mouse_hover_y = match self.get_entry(y) {
                        Entry::Mod(_)
                        | Entry::Builtin(_) => Some(y),
                        _ => None,
                    };
                } else if self.mouse_hover_y.is_some(){
                    self.mouse_hover_y = None;
                }

                if self.update_mouse_drag(drag) || self.update_mouse_hover(hover) {
                    control.redraw();
                }
            }

            EventKind::MouseLeftRelease
            | EventKind::MouseRightRelease => {
                let is_right = event.kind == EventKind::MouseRightRelease;
                if let Some(clicked) = self.clicked_mod {
                    control.release_mouse();
                    if self.mouse_drag_y.is_none()
                        && let Entry::Mod(entry) = self.get_entry(y)
                        && entry == clicked
                    {
                        if let Some(no_clear) = self.select_defer.take() {
                            if no_clear {
                                let check = self.selected.iter()
                                    .enumerate()
                                    .find(|(_, c)| **c == clicked);

                                if let Some((i, _)) = check {
                                    self.selected.remove(i);
                                } else {
                                    self.selected.push(clicked);
                                }
                            } else {
                                self.selected.clear();
                                self.selected.push(clicked);
                            }

                            control.redraw();
                        }
                    } else {
                        let (swap_to, _) = self.get_slot(y);
                        if self.move_selected(swap_to) {
                            if clicked < swap_to {
                                self.selected_pivot = swap_to - 1;
                            } else {
                                self.selected_pivot = swap_to;
                            }
                            if is_inside {
                                self.mouse_hover_y = Some(y);
                            }
                            self.update_mod_lorder();
                            control.redraw();
                        }
                    }
                }

                if self.dropdown_defer
                    && is_right
                    && self.mouse_drag_y.is_none()
                {
                    self.mouse_hover_y = None;
                    control.move_widget(Control::DROPDOWN_WIDGET, x, y);
                    control.show_widget(Control::DROPDOWN_WIDGET);
                    control.redraw();
                }

                self.dropdown_defer = false;
                self.clicked_mod = None;
                self.mouse_drag_y = None;
                self.select_defer = None;

                if self.update_mouse_hover(is_inside.then_some(y)) {
                    control.redraw();
                }
            }

            //(EventKind::LostFocus, _) => {
            //    self.clicked_mod = None;
            //    self.mouse_drag_y = None;
            //    self.mouse_hover_mod = None;
            //}

            EventKind::MouseLeftPress
            | EventKind::MouseRightPress => {
                let is_right = event.kind == EventKind::MouseRightPress;
                if is_inside {
                    self.clicked_mod = if let Entry::Mod(clicked) = self.get_entry(y) {
                        if !(event.shift || event.ctrl || self.selected.contains(&clicked)) {
                            self.selected.clear();
                        }

                        if is_right {
                            self.dropdown_defer = true;
                            if event.ctrl {
                                if !event.shift {
                                    self.selected_pivot = clicked;
                                }
                            } else if !self.selected.contains(&clicked) {
                                if event.shift {
                                    self.select_defer = Some(false);
                                } else {
                                    self.selected.clear();
                                    self.selected.push(clicked);
                                }
                            }
                        } else if self.dropdown_defer {
                            self.dropdown_defer = false;
                            self.select_defer = None;

                            if event.shift {
                                self.selected.clear();
                                self.selected.push(clicked);
                            }

                            self.mouse_hover_y = None;
                            control.move_widget(Control::DROPDOWN_WIDGET, x, y);
                            control.show_widget(Control::DROPDOWN_WIDGET);
                            control.redraw();
                        } else if event.shift {
                            let min = self.selected_pivot.min(clicked);
                            let max = self.selected_pivot.max(clicked);

                            if event.ctrl {
                                if !self.selected.contains(&self.selected_pivot) {
                                    self.selected.retain(|&i| i < min || i > max);
                                } else {
                                    for i in min..=max {
                                        if !self.selected.contains(&i) {
                                            self.selected.push(i);
                                        }
                                    }
                                }
                            } else {
                                self.selected.clear();
                                for i in min..=max {
                                    self.selected.push(i);
                                }
                            }
                        } else {
                            self.selected_pivot = clicked;
                            if self.selected.contains(&clicked) || event.ctrl {
                                self.select_defer = Some(event.ctrl);
                            } else {
                                self.selected.push(clicked);
                            }
                        }

                        control.redraw();
                        control.capture_mouse();
                        Some(clicked)
                    } else {
                        if !(event.shift || event.ctrl || self.selected.is_empty()) {
                            self.selected.clear();
                            control.redraw();
                        }

                        None
                    };
                } else if !(event.ctrl || event.shift || self.selected.is_empty()) {
                    self.selected.clear();
                    control.redraw();
                }
            }

            EventKind::MouseDoubleClick => {
                if is_inside
                    && let Entry::Mod(entry) = self.get_entry(y)
                    && self.toggle_mod(entry, None)
                {
                    self.update_mod_lorder();
                    control.redraw();
                }
            }

            EventKind::MouseScroll(delta) if delta != 0 => {
                let mut scroll = self.scroll;
                if delta < 0 {
                    let bottom = self.scroll + Self::HEIGHT_INNER as i32;
                    scroll += self.item_height;
                    let diff = bottom % self.item_height;
                    if diff != 0 {
                        scroll += self.item_height - diff;
                    }
                } else {
                    scroll = scroll.saturating_sub(self.item_height + self.scroll % self.item_height);
                }

                let bottom_item = (scroll + Self::HEIGHT_INNER as i32 + self.item_height - 1) / self.item_height;
                let max_item = i32::try_from(self.builtins.len() + self.lorder.mods.len()).unwrap();
                if scroll >= 0 && scroll != self.scroll && bottom_item <= max_item {
                    self.scroll = scroll;
                    control.redraw();
                }
            }

            EventKind::KeyDown(KeyKind::Space) => {
                if !self.selected.is_empty() {
                    let mods = &mut self.lorder.mods;
                    let mut all_enabled = true;
                    for i in &self.selected {
                        if let Some(m) = mods.get(*i) {
                            if self.using_aml {
                                match m.state {
                                    ModState::Enabled
                                    | ModState::MissingEntry => (),
                                    ModState::Disabled => all_enabled = false,
                                    ModState::NotInstalled => (),
                                }
                            } else {
                                match m.state {
                                    ModState::Enabled => (),
                                    ModState::Disabled => all_enabled = false,
                                    ModState::MissingEntry => (),
                                    ModState::NotInstalled => (),
                                }
                            };
                        }
                    }

                    for i in &self.selected {
                        if let Some(m) = mods.get_mut(*i) {
                            match (all_enabled, m.state.clone()) {
                                (true, ModState::Enabled) => m.state = ModState::Disabled,
                                (false, ModState::Disabled) => m.state = ModState::Enabled,
                                _ => (),
                            }
                        }
                    }

                    self.update_mod_lorder();
                    control.redraw();
                }
            }

            EventKind::Hide => control.hide_widget(Control::DROPDOWN_WIDGET),

            _ => (),
        }
    }

    fn render(&mut self, context: &mut super::DrawScope) {
        context.draw_bitmap(&self.background, None, None);

        let left = Self::MARGIN_X;
        let top = Self::MARGIN_Y;
        let right = Self::MARGIN_X + Self::WIDTH_INNER;
        let bottom = Self::MARGIN_Y + Self::HEIGHT_INNER;
        context.push_axis_aligned_clip(&[
            left as f32,
            top as f32,
            right as f32,
            bottom as f32,
        ]);

        let start = self.scroll / self.item_height;
        let mut start = usize::try_from(start).unwrap();
        let mut offset = self.scroll % self.item_height;
        if offset != 0 {
            offset -= self.item_height;
        }

        if start < self.builtins.len() {
            for (i, builtin) in self.builtins[start..].iter().enumerate() {
                let i = i + start;

                self.draw_mod(
                    context,
                    builtin,
                    Self::MOD_BUILTIN_GOLD,
                    offset as u32,
                    Some(Entry::Builtin(i)) == self.mouse_hover_y.map(|y| self.get_entry(y)),
                    false,
                );
                offset += self.item_height;
            }
        }
        start = start.saturating_sub(self.builtins.len());

        let mods = &self.lorder.mods;
        if mods.len() > start {
            for (i, m) in mods[start..].iter().enumerate() {
                let i = i + start;
                if offset >= Self::HEIGHT_INNER as i32 {
                    break;
                }

                let color = if self.using_aml {
                    match m.state {
                        ModState::Enabled
                        | ModState::MissingEntry => Self::MOD_ENABLED_BLUE,
                        ModState::Disabled => Self::MOD_DISABLED_GRAY,
                        ModState::NotInstalled => {
                            // TODO: log bad state
                            continue;
                        }
                    }
                } else {
                    match m.state {
                        ModState::Enabled => Self::MOD_ENABLED_BLUE,
                        ModState::Disabled => Self::MOD_DISABLED_GRAY,
                        ModState::MissingEntry => Self::MOD_MISSING_ENTRY_ORANGE,
                        ModState::NotInstalled => Self::MOD_NOT_INSTALLED_RED,
                    }
                };

                self.draw_mod(
                    context,
                    m.name(),
                    color,
                    offset as u32,
                    Some(Entry::Mod(i)) == self.mouse_hover_y.map(|y| self.get_entry(y)),
                    self.selected.contains(&i),
                );
                offset += self.item_height;
            }
        }

        context.pop_axis_aligned_clip();

        if let Some(drag_y) = self.mouse_drag_y {
            unsafe {
                self.brush.SetColor(Self::MOD_BUILTIN_GOLD.as_ptr() as *const _);
            }

            let (_, draw_y) = self.get_slot(drag_y);
            let from = [
                Self::MARGIN_X as f32,
                draw_y as f32,
            ];
            let to = [
                Self::MARGIN_X as f32 + Self::MOD_ENTRY_LENGTH,
                draw_y as f32,
            ];
            context.draw_line(from, to, &self.brush, 3.0);
        }
    }
}
