use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;
use std::io;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::sync::mpsc::Receiver;

use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use crate::dxgi::SolidColorBrush;
use crate::dxgi::TextFormat;

use crate::mod_engine::ModEngine;
use crate::mod_engine::ModState;
use crate::archive::Archive;
use crate::archive::ArchiveList;
use crate::archive::ArchiveView;
use crate::archive::Prefix;
use super::Control;
use super::WidgetConfig;
use super::button;
use super::button::ButtonWidget;
use super::dropdown::DropdownMenu;
use super::dropdown::DropdownWidget;
use super::Event;
use super::EventKind;
use super::KeyKind;

fn check_archive(_path: &Path, list: &ArchiveList) -> io::Result<Prefix> {
    if list.list("mods").is_some()
        || list.list("binaries").is_some()
    {
        return Ok(Prefix::None);
    } else {
        let mut parent = None;
        for (path, _ty, depth) in list.iter() {
            if depth == 0 {
                parent = Some(path);
            } else if depth == 1
                && let Some(name) = path.strip_suffix(".mod")
                && Some(name) == parent
            {
                return Ok(Prefix::Mods);
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::Other, "unknown layout from dragdrop archive"))
}

enum DragDropEvent {
    Error(String),
    List(ArchiveView),
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DragDropState {
    None,
    Listing,
    Dragging,
    Copying,
    Copied,
}

struct DragDrop {
    state: DragDropState,
    root: PathBuf,
    pump: Option<(Sender<DragDropEvent>, Receiver<DragDropEvent>)>,
    archive: Option<Archive>,
    view: Option<ArchiveView>,
    complete: Option<Box<dyn FnOnce() + Send + Sync>>,
    error: Option<String>,
}

impl DragDrop {
    fn new(root: &Path) -> Self {
        Self {
            state: DragDropState::None,
            root: root.canonicalize().unwrap(),
            pump: None,
            archive: None,
            view: None,
            complete: None,
            error: None,
        }
    }

    fn clear(&mut self) -> bool {
        let redraw = self.state != DragDropState::None
            || self.pump.is_some()
            || self.archive.is_some()
            || self.view.is_some();
        self.state = DragDropState::None;
        self.pump = None;
        self.archive = None;
        self.view = None;
        redraw
    }

    fn poll(&mut self) -> bool {
        if let Some((_send, recv)) = &self.pump {
            let mut new_state = self.state;
            while let Ok(event) = recv.try_recv() {
                new_state = match event {
                    DragDropEvent::Error(err) => {
                        crate::log::log(&err);
                        self.error = Some(err);
                        DragDropState::None
                    }
                    DragDropEvent::List(view) => {
                        self.view = Some(view);
                        if self.state == DragDropState::Copying {
                            self.state
                        } else {
                            DragDropState::Dragging
                        }
                    }
                    DragDropEvent::Copy => DragDropState::Copied,
                }
            }

            if new_state != self.state {
                let old_state = self.state;
                self.state = new_state;
                match self.state {
                    DragDropState::None => {
                        self.clear();
                        if old_state != DragDropState::Copying {
                            self.state = DragDropState::Dragging
                        }
                    }
                    DragDropState::Copying => {
                        assert!(self.view.is_some());
                        self.copy();
                    }
                    _ => (),
                }

                true
            } else {
                false
            }
        } else {
            false
        }
    }

    fn format_error(err: &io::Error) -> String {
        if let Some(inner) = err.get_ref() {
            match err.kind() {
                io::ErrorKind::Other => format!("modtide error:\n  {inner:?}"),
                kind => format!("{kind}:\n  {inner:?}"),
            }
        } else {
            format!("{err:?}")
        }
    }

    fn copy(&mut self) {
        if let Some((send, _recv)) = &self.pump {
            if self.is_dragging() {
                let view = self.view.as_mut().unwrap();
                let complete = self.complete.take().unwrap();
                let send = send.clone();
                view.copy(&self.root, move |count| {
                    let _ = match count {
                        Ok(_count) => send.send(DragDropEvent::Copy),
                        Err(err) => send.send(DragDropEvent::Error(Self::format_error(&err))),
                    };
                    complete();
                });
            }
        } else {
            self.state = DragDropState::None;
        }
    }

    fn is_dragging(&self) -> bool {
        matches!(self.state, DragDropState::Listing | DragDropState::Dragging)
    }

    fn mouse_enter(
        &mut self,
        files: &[PathBuf],
        complete: impl FnOnce() + Send + Sync + 'static,
    ) {
        self.clear();
        // see DragDrop::mouse_leave
        //assert!(matches!(self.state, DragDropState::None | DragDropState::Copied));
        self.error = None;

        match Archive::new(files, check_archive) {
            Ok(archive) => {
                let (send, recv) = mpsc::channel();
                let send_ = send.clone();
                archive.view(move |view| {
                    let _ = match view {
                        Ok(view) => send_.send(DragDropEvent::List(view)),
                        Err(err) if err.kind() == io::ErrorKind::WouldBlock => return,
                        Err(err) => send_.send(DragDropEvent::Error(Self::format_error(&err))),
                    };
                    complete();
                });
                self.state = DragDropState::Listing;
                self.pump = Some((send, recv));
                self.archive = Some(archive);
            }
            Err(err) => {
                self.error = Some(Self::format_error(&err));
                self.clear();
                self.state = DragDropState::Dragging;
            }
        }
    }

    // TODO: fix Control MouseLeave to work the same between windows and wine
    //fn mouse_leave(&mut self) -> bool {
    //    if self.is_dragging() {
    //        self.clear();
    //        true
    //    } else {
    //        false
    //    }
    //}

    fn drag_drop(
        &mut self,
        complete: impl FnOnce() + Send + Sync + 'static,
    ) {
        self.complete = Some(Box::new(complete));
        self.copy();
    }
}

#[derive(Clone)]
pub enum ModListEvent {
    ToggleSelected = 0,
    OpenSelected = 1,
    DragDropPoll = 2,
}

impl ModListEvent {
    fn from_u32(msg: u32) -> Option<Self> {
        Some(match msg {
            0 => ModListEvent::ToggleSelected,
            1 => ModListEvent::OpenSelected,
            2 => ModListEvent::DragDropPoll,
            _ => return None,
        })
    }
}

pub struct ModListWidget {
    background: ID2D1Bitmap,
    brush: SolidColorBrush,
    text_format: TextFormat,

    mods_path: PathBuf,
    lorder: ModEngine,
    builtins: Vec<&'static str>,
    using_aml: bool,

    scroll: i32,
    item_height: i32,
    active_mod: usize,
    clicked_mod: Option<usize>,
    mouse_pos: (i32, i32),
    can_drag: bool,
    can_hover: bool,
    selected: Vec<usize>,
    selected_pivot: usize,
    select_defer: Option<bool>,
    dropdown_defer: bool,

    drag_drop: DragDrop,
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
        brush: SolidColorBrush,
        text_format: TextFormat,
    ) -> Self {
        let mods_path = mods_path.into();
        let drag_drop = DragDrop::new(mods_path.parent().unwrap());
        Self {
            background,
            brush,
            text_format,

            mods_path,
            lorder: ModEngine::new(),
            builtins: Vec::new(),
            using_aml: false,

            scroll: 0,
            item_height: Self::ITEM_HEIGHT as i32,
            active_mod: usize::MAX,
            clicked_mod: None,
            mouse_pos: (-1, -1),
            can_drag: false,
            can_hover: false,
            selected: Vec::new(),
            selected_pivot: 0,
            select_defer: None,
            dropdown_defer: false,

            drag_drop,
        }
    }

    pub fn fallback(
        context: &mut super::DrawScope,
        brush: &SolidColorBrush,
    ) {
        let rect = [
            (Self::MARGIN_X - 2) as f32,
            (Self::MARGIN_Y - 2) as f32,
            (Self::MARGIN_X + 2 + Self::WIDTH_INNER) as f32,
            (Self::MARGIN_Y + 2 + Self::HEIGHT_INNER) as f32,
        ];
        let radius = 8.0;

        brush.set_color(&Self::FALLBACK_BACKGROUND);
        context.fill_rounded_rect(
            brush,
            rect,
            radius,
        );

        brush.set_color(&Self::FALLBACK_BORDER);
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

    fn get_entry(&self, pos: (i32, i32)) -> Entry {
        let (x, y) = pos;
        let left = Self::MARGIN_X as i32;
        let top = Self::MARGIN_Y as i32;
        let offset = y - top;
        if offset < 0
            || offset > Self::HEIGHT_INNER as i32
            || x < left
            || x - left > Self::MOD_ENTRY_LENGTH as i32
        {
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

    fn get_slot(&self, pos: (i32, i32)) -> (usize, u32) {
        let y = pos.1;
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

    fn toggle_selected(&mut self) -> bool {
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

            true
        } else {
            false
        }
    }

    fn open_selected(&self) {
        use std::os::windows::ffi::OsStrExt;

        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::CreateProcessW;
        use windows::Win32::System::Threading::STARTUPINFOW;

        for i in &self.selected {
            let Some(m) = self.lorder.mods.get(*i) else {
                continue;
            };

            if m.state == ModState::NotInstalled {
                continue;
            }

            let Ok(path) = self.mods_path.join(m.path()).canonicalize() else {
                continue;
            };

            let osstr = path.into_os_string();
            let mut cmd = b"C:\\Windows\\explorer.exe \"".map(|b| b as u16).to_vec();
            let mut wide = osstr.encode_wide();
            if osstr.as_encoded_bytes().starts_with(b"\\\\?\\") {
                let _ = wide.nth(3);
            }
            cmd.extend(wide);
            cmd.push(b'"' as u16);
            cmd.push(0);

            let info = STARTUPINFOW {
                cb: core::mem::size_of::<STARTUPINFOW>() as u32,
                ..Default::default()
            };
            let mut out = Default::default();
            unsafe {
                if CreateProcessW(
                    windows::core::w!(r"C:\Windows\explorer.exe"),
                    Some(windows::core::PWSTR(cmd.as_mut_ptr())),
                    None,
                    None,
                    false,
                    Default::default(),
                    None,
                    None,
                    &info,
                    &mut out,
                ).is_ok() {
                    let _ = CloseHandle(out.hProcess);
                    let _ = CloseHandle(out.hThread);
                }
            }
        }
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
            self.brush.set_color(&Self::MOD_HIGHLIGHT);

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

        self.brush.set_color(&color);

        let rect = [
            (left + Self::TEXT_PADDING) as f32,
            (top + o) as f32,
            left as f32 + Self::MOD_ENTRY_LENGTH,
            (top + o + item_height) as f32,
        ];
        context.draw_text(
            text.as_ref(),
            &self.text_format,
            &self.brush,
            &rect,
        );

        if selected {
            self.brush.set_color(&color);

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

    fn update_mouse(
        &mut self,
        pos: (i32, i32),
    ) -> bool {
        let old_pos = self.mouse_pos;
        if pos != old_pos {
            self.mouse_pos = pos;

            if self.can_hover {
                if let Some(clicked) = self.clicked_mod
                    && let entry = self.get_entry(pos)
                    && (entry != Entry::Mod(clicked) || entry == Entry::None)
                {
                    self.can_hover = false;
                    self.can_drag = true;
                    return true;
                } else if self.get_entry(pos) != self.get_entry(old_pos) {
                    return true;
                }
            } else if self.can_drag {
                let (_, slot1) = self.get_slot(pos);
                let (_, slot2) = self.get_slot(old_pos);
                if slot1 != slot2 {
                    return true;
                }
            }
        }

        false
    }

    pub fn send(
        control: &mut super::ControlScope,
        event: ModListEvent,
    ) {
        control.send_event(Control::MOD_LIST_WIDGET, event as u32);
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
        if let EventKind::Custom(custom) = event.kind {
            if let Some(event) = ModListEvent::from_u32(custom) {
                match event {
                    ModListEvent::ToggleSelected => {
                        if self.toggle_selected() {
                            self.update_mod_lorder();
                            control.redraw();
                        }
                    }
                    ModListEvent::OpenSelected => self.open_selected(),
                    ModListEvent::DragDropPoll => {
                        if self.drag_drop.poll() {
                            if self.drag_drop.state == DragDropState::Copied {
                                self.mount().unwrap();

                                if let Some(view) = &self.drag_drop.view
                                    && let Some(mods) = view.list().list("mods")
                                {
                                    let mut enable = Vec::new();
                                    for (name, ty, depth) in mods.iter() {
                                        if depth == 0 && ty.is_dir() {
                                            let res = self.lorder.mods.iter()
                                                .enumerate()
                                                .find(|(_, m)| m.name() == name && m.state == ModState::Disabled);
                                            if let Some((i, _)) = res {
                                                enable.push(i);
                                            }
                                        }
                                    }

                                    for i in &enable {
                                        self.toggle_mod(*i, Some(true));
                                    }
                                    if !enable.is_empty() {
                                        self.update_mod_lorder();
                                    }
                                }
                            }

                            control.redraw();
                        }
                    }
                }
            }
            return;
        }

        let x = event.x;
        let y = event.y;

        let left = Self::MARGIN_X as i32;
        let top = Self::MARGIN_Y as i32;
        let right = left + Self::WIDTH_INNER as i32;
        let bottom = top + Self::HEIGHT_INNER as i32;

        let is_inside = x >= left && x < right
            && y >= top && y < bottom;

        match event.kind {
            EventKind::MouseEnter(true) => {
                let notify = control.dispatcher();
                let drag_files = control.drag_files().unwrap();
                self.drag_drop.mouse_enter(drag_files, move || {
                    notify(ModListEvent::DragDropPoll as u32);
                });
                control.redraw();
            }

            EventKind::MouseLeave => {
                if self.update_mouse(self.mouse_pos) {
                    control.redraw();
                }
            }

            EventKind::MouseMove(is_dragging) => {
                if !self.can_drag {
                    self.can_hover = !is_dragging;
                }

                if self.update_mouse((x, y)) {
                    control.redraw();
                }
            }

            EventKind::MouseLeftRelease if self.dropdown_defer => (),
            EventKind::MouseLeftRelease
            | EventKind::MouseRightRelease => {
                let is_right = event.kind == EventKind::MouseRightRelease;
                if let Some(clicked) = self.clicked_mod {
                    control.release_mouse();
                    if !self.can_drag
                        && Entry::Mod(clicked) == self.get_entry((x, y))
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

                        if self.dropdown_defer && is_right {
                            self.can_hover = true;
                            DropdownWidget::show(control, x, y, DropdownMenu::ModSelected);
                            control.redraw();
                        }
                    } else {
                        let (swap_to, _) = self.get_slot((x, y));
                        if self.move_selected(swap_to) {
                            if clicked < swap_to {
                                self.selected_pivot = swap_to - 1;
                            } else {
                                self.selected_pivot = swap_to;
                            }
                            if is_inside {
                                self.can_hover = true;
                            }
                            self.update_mod_lorder();
                            control.redraw();
                        }
                    }
                }

                if event.kind == EventKind::MouseRightRelease {
                    self.dropdown_defer = false;
                }
                self.clicked_mod = None;
                self.can_drag = false;
                self.select_defer = None;

                if self.update_mouse(self.mouse_pos) {
                    control.redraw();
                }
            }

            //(EventKind::LostFocus, _) => {
            //    self.clicked_mod = None;
            //    self.mouse_drag_y = None;
            //    self.mouse_hover_mod = None;
            //}

            EventKind::MouseLeftPress if self.dropdown_defer => (),
            EventKind::MouseLeftPress
            | EventKind::MouseRightPress => {
                let is_right = event.kind == EventKind::MouseRightPress;
                if is_inside {
                    self.clicked_mod = if let Entry::Mod(clicked) = self.get_entry(self.mouse_pos) {
                        if !(event.shift || event.ctrl || self.selected.contains(&clicked)) {
                            self.selected.clear();
                        }

                        if is_right {
                            self.dropdown_defer = true;
                            if !event.ctrl || event.shift {
                                self.selected_pivot = clicked;
                                if !self.selected.contains(&clicked) {
                                    self.selected_pivot = clicked;
                                    self.selected.clear();
                                    self.selected.push(clicked);
                                }
                            }
                        //} else if self.dropdown_defer {
                        //    self.dropdown_defer = false;
                        //    self.select_defer = None;
                        //
                        //    if event.shift {
                        //        self.selected.clear();
                        //        self.selected.push(clicked);
                        //    }
                        //
                        //    self.mouse_hover_y = None;
                        //    DropdownWidget::show(control, x, y, DropdownMenu::ModSelected);
                        //    control.redraw();
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
                        self.active_mod = clicked;
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
                    && !self.dropdown_defer
                    && Entry::Mod(self.active_mod) == self.get_entry(self.mouse_pos)
                    && !self.selected.is_empty()
                {
                    self.toggle_selected();
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

            EventKind::KeyDown(key) => {
                match key {
                    KeyKind::Space => {
                        if self.toggle_selected() {
                            self.update_mod_lorder();
                            control.redraw();
                        }
                    }
                    KeyKind::Escape => {
                        self.dropdown_defer = false;
                        self.clicked_mod = None;
                        self.can_drag = false;
                        self.can_hover = is_inside;
                        self.select_defer = None;
                        self.drag_drop.clear();
                        self.drag_drop.error = None;
                        control.redraw();
                    }
                }
            }

            EventKind::Hide => DropdownWidget::hide(control),

            EventKind::DragDrop => {
                let notify = control.dispatcher();
                self.drag_drop.drag_drop(move || {
                    notify(ModListEvent::DragDropPoll as u32);
                });
                control.redraw();
            }

            _ => (),
        }
    }

    fn render(&mut self, context: &mut super::DrawScope) {
        context.draw_bitmap(&self.background, None, None);

        self.text_format.set_word_wrapping(crate::dxgi::WordWrapping::NoWrap).unwrap();

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
                    Some(Entry::Builtin(i)) == self.can_hover.then(|| self.get_entry(self.mouse_pos)),
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
                    Some(Entry::Mod(i)) == self.can_hover.then(|| self.get_entry(self.mouse_pos)),
                    self.selected.contains(&i),
                );
                offset += self.item_height;
            }
        }

        context.pop_axis_aligned_clip();

        if self.drag_drop.is_dragging() {
            self.brush.set_color(&[0.0, 0.0, 0.0, 0.5]);
            context.fill_rounded_rect(
                &self.brush,
                [left, top, right, bottom].map(|b| b as f32),
                0.0,
            );
        }

        if self.can_drag {
            self.brush.set_color(&Self::MOD_BUILTIN_GOLD);

            let (_, draw_y) = self.get_slot(self.mouse_pos);
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

        if let Some(view) = &self.drag_drop.view {
            let item_height = self.item_height as u32;
            let left = left + Self::MOD_ENTRY_LENGTH as u32 + 16;
            let top = top + item_height;
            let right = right - 8;
            let bottom = bottom - item_height;

            context.push_axis_aligned_clip(&[
                left as f32,
                top as f32,
                right as f32,
                bottom as f32,
            ]);

            self.brush.set_color(&[0.7, 0.7, 0.7, 1.0]);

            let mut offset = top;
            let mut in_mods = false;
            let mut text = String::new();
            for (name, ty, depth) in view.list().iter() {
                if offset >= bottom {
                    break;
                }

                if depth == 0 {
                    in_mods = name == "mods";
                } else if in_mods && depth > 1 {
                    continue;
                }

                let text = if (in_mods && depth > 0) || !ty.is_dir() {
                    name
                } else {
                    text.clear();
                    text.push_str(name);
                    text.push_str("/");
                    &text
                };

                let depth = depth as u32 * 8;

                let rect = [
                    (left + depth) as f32,
                    offset as f32,
                    right as f32,
                    (offset + item_height) as f32,
                ];
                context.draw_text(
                    text.as_ref(),
                    &self.text_format,
                    &self.brush,
                    &rect,
                );
                offset += item_height;
            }

            context.pop_axis_aligned_clip();
        } else if let Some(text) = &self.drag_drop.error {
            let item_height = self.item_height as u32;
            let left = left + Self::MOD_ENTRY_LENGTH as u32 + 16;
            let top = top + item_height;
            let right = right - 8;
            let bottom = bottom - item_height;

            self.brush.set_color(&[0.8, 0.2, 0.2, 1.0]);
            self.text_format.set_word_wrapping(crate::dxgi::WordWrapping::Wrap).unwrap();

            context.draw_text(
                text.as_ref(),
                &self.text_format,
                &self.brush,
                &[left, top, right, bottom].map(|b| b as f32),
            );
        }
    }
}
