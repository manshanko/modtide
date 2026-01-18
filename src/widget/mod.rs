use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use std::path::PathBuf;

use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::dxgi::DrawScope;

pub mod button;
pub mod list;
pub mod dropdown;
mod drop_target;

pub trait Widget: Send + 'static {
    fn config(&self) -> WidgetConfig {
        Default::default()
    }

    fn rect(&self, width: u32, height: u32) -> [u32; 4];

    fn hit_test(&self, _x: u32, _y: u32) -> bool {
        true
    }

    fn handle_event(
        &mut self,
        control: &mut ControlScope,
        event: Event,
    );

    fn render(&mut self, context: &mut DrawScope);
}

#[derive(Default)]
pub struct WidgetConfig {
    listen_double_click: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyKind {
    Space,
    Escape,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EventKind {
    MouseMove(bool),
    MouseLeftPress,
    MouseLeftRelease,
    MouseRightPress,
    MouseRightRelease,
    MouseDoubleClick,
    MouseScroll(i32),
    MouseEnter(bool),
    MouseLeave,
    KeyDown(KeyKind),
    LostFocus,
    Show,
    Hide,
    DragDrop,
    Custom(u32),
    None,
}

impl EventKind {
    fn is_dragdrop(&self) -> bool {
        matches!(self, EventKind::MouseMove(true) | EventKind::DragDrop)
    }
}

#[derive(Clone)]
pub struct Event {
    pub kind: EventKind,
    pub ctrl: bool,
    pub shift: bool,
    pub x: i32,
    pub y: i32,
}

impl Event {
    fn from_msg(hwnd: &HWND, msg: u32, w_param: usize) -> Option<Self> {
        let kind = match msg {
            //WM_MOUSELEAVE
            //675 => EventKind::MouseMove,
            WM_SETCURSOR => EventKind::MouseMove(false),
            WM_MOUSEMOVE => EventKind::MouseMove(false),
            WM_LBUTTONDOWN => EventKind::MouseLeftPress,
            WM_LBUTTONUP => EventKind::MouseLeftRelease,
            WM_RBUTTONDOWN => EventKind::MouseRightPress,
            WM_RBUTTONUP => EventKind::MouseRightRelease,
            WM_MOUSEWHEEL => {
                let delta = (w_param >> 16) as i16;
                EventKind::MouseScroll(delta as i32 / WHEEL_DELTA as i32)
            }
            WM_KEYDOWN => {
                let Ok(key) = u16::try_from(w_param) else {
                    return None;
                };
                let kind = match VIRTUAL_KEY(key) {
                    VK_SPACE => KeyKind::Space,
                    VK_ESCAPE => KeyKind::Escape,
                    _ => return None,
                };
                EventKind::KeyDown(kind)
            }
            _ => return None,
        };

        let mut ctrl = false;
        let mut shift = false;
        if kind == EventKind::MouseLeftPress
            || kind == EventKind::MouseRightPress
        {
            ctrl = w_param & 0x0008 /*MK_CONTROL*/ != 0;
            shift = w_param & 0x0004 /*MK_SHIFT*/ != 0;
        }

        let mut pt = POINT {
            x: -1,
            y: -1,
        };
        unsafe {
            let mut rect = RECT::default();
            if GetCursorPos(&mut pt).is_ok()
                && GetWindowRect(*hwnd, &mut rect).is_ok()
            {
                pt.x -= rect.left;
                pt.y -= rect.top;
            }
        }
        let x = pt.x;
        let y = pt.y;

        Some(Self {
            kind,
            ctrl,
            shift,
            x,
            y,
        })
    }

    fn can_capture(msg: u32) -> bool {
        match msg {
            WM_LBUTTONDOWN => true,
            //WM_LBUTTONUP => true,
            WM_MOUSEWHEEL => true,
            _ => false,
        }
    }

    fn breaks_capture(&self) -> Option<bool> {
        Some(match self.kind {
            EventKind::MouseLeftPress
            | EventKind::MouseRightPress => false,
            EventKind::KeyDown(KeyKind::Escape) => true,
            _ => return None,
        })
    }

    fn scope(&self, rect: [u32; 4]) -> Self {
        let mut out = self.clone();
        out.x -= rect[0] as i32;
        out.y -= rect[1] as i32;
        out
    }
}

impl Default for Event {
    fn default() -> Self {
        Self {
            kind: EventKind::None,
            ctrl: false,
            shift: false,
            x: -1,
            y: -1,
        }
    }
}

enum WidgetEvent {
    Toggle(usize),
    Hide(usize),
    Show(usize),
    Move(usize, usize, i32, i32),
    Resize(usize, u32, u32),
    CaptureMouse(Option<usize>),
    SendEvent(usize, u32),
    Redraw,
}

struct WidgetState {
    inner: Box<dyn Widget>,
    config: WidgetConfig,
    rect: [u32; 4],
    visible: bool,
}

impl WidgetState {
    fn new(inner: Box<dyn Widget>, visible: bool) -> Self {
        Self {
            config: inner.config(),
            inner,
            rect: [0; 4],
            visible,
        }
    }
}

pub struct Control {
    hwnd: HWND,
    pub display: HWND,
    capture_mouse: Option<usize>,
    last: Option<usize>,
    widgets: Vec<WidgetState>,
    events: Vec<WidgetEvent>,

    dirty: bool,

    clicked: Option<(usize, Instant, i32, i32)>,
    dbl_click_msec: Duration,
    dbl_click_width: i32,
    dbl_click_height: i32,
    drag_files: Option<Vec<PathBuf>>,

    hooks: Vec<(HWND, unsafe extern "system" fn(
        hwnd: HWND,
        msg: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT)>,
}

unsafe impl Send for Control {}
unsafe impl Sync for Control {}

impl Control {
    pub const MOD_LIST_WIDGET: usize = 0;
    //pub const BUTTON_WIDGET: usize = 1;
    pub const DROPDOWN_WIDGET: usize = 2;

    const WM_PRIV_MOUSE: u32 = WM_APP + 0x333;
    const WM_PRIV_MOUSELEAVE: u32 = WM_APP + 0x334;
    const WM_PRIV_DRAGENTER: u32 = WM_APP + 0x335;
    const WM_PRIV_DRAGMOVE: u32 = WM_APP + 0x336;
    const WM_PRIV_DRAGDROP: u32 = WM_APP + 0x337;

    pub fn hook(
        mod_list: list::ModListWidget,
        button: button::ButtonWidget,
        dropdown: dropdown::DropdownWidget,
        hwnd: HWND,
    ) {
        let mut control = CONTROL.lock().unwrap();
        assert!(control.is_none(), "only one hooked instance supported");

        let mut rect;
        unsafe {
            rect = core::mem::zeroed();
            GetWindowRect(hwnd, &mut rect).unwrap();
        }
        let width = u32::try_from(rect.right - rect.left).unwrap();
        let height = u32::try_from(rect.bottom - rect.top).unwrap();

        let mut widgets = Vec::new();
        widgets.push(WidgetState::new(Box::new(mod_list), cfg!(debug_assertions)));
        widgets.push(WidgetState::new(Box::new(button), true));
        widgets.push(WidgetState::new(Box::new(dropdown), false));

        for widget in &mut widgets {
            widget.rect = widget.inner.rect(width, height);
            assert!(widget.rect[0] <= widget.rect[2]);
            assert!(widget.rect[1] <= widget.rect[3]);
        }

        let mut hooks = Vec::new();
        let mut display = None;
        unsafe {
            let current_proc_id = windows::Win32::System::Threading::GetCurrentProcessId();
            for wnd_name in [
                w!("Launcher"),
                w!("Alpha"),
            ] {
                if let Ok(hwnd) = FindWindowW(None, wnd_name) {
                    let mut proc_id =0;
                    GetWindowThreadProcessId(hwnd, Some(&mut proc_id));
                    assert!(proc_id == current_proc_id);

                    let hook = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc as *const () as isize);
                    if hook != 0 {
                        hooks.push((hwnd, core::mem::transmute(hook)));
                    }

                    let hwnd_ = hwnd.0 as usize;
                    crate::panic::on_unwind(move || {
                        let hwnd = HWND(hwnd_ as *mut _);
                        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, hook);
                        update_display(&hwnd);
                    });

                    display = Some(hwnd);
                }
            }
        }
        let display = display.unwrap_or(hwnd);

        let dbl_click_msec;
        let dbl_click_width;
        let dbl_click_height;
        unsafe {
            dbl_click_msec = Duration::from_millis(GetDoubleClickTime() as u64);
            dbl_click_width = GetSystemMetrics(SM_CXDOUBLECLK);
            dbl_click_height = GetSystemMetrics(SM_CYDOUBLECLK);
        }

        *control = Some(Control {
            hwnd,
            display,
            capture_mouse: None,
            last: None,
            widgets,
            events: Vec::new(),

            dirty: false,

            clicked: None,
            dbl_click_msec,
            dbl_click_width,
            dbl_click_height,
            drag_files: None,

            hooks,
        });

        GlobalMouseHook::start(hwnd);
        drop_target::DropTarget::start(hwnd, display);
    }

    fn test_widgets(&self, x: i32, y: i32) -> Option<usize> {
        let x = u32::try_from(x).ok()?;
        let y = u32::try_from(y).ok()?;

        for i in 0..self.widgets.len() {
            let i = self.widgets.len() - 1 - i;
            let widget = &self.widgets[i];
            if !widget.visible {
                continue;
            }

            let x0 = widget.rect[0];
            let y0 = widget.rect[1];
            let x1 = widget.rect[2];
            let y1 = widget.rect[3];
            if x >= x0 && x < x1
                && y >= y0 && y < y1
                && widget.inner.hit_test(x - x0, y - y0)
            {
                return Some(i);
            }
        }

        None
    }

    fn mouse_leave(&mut self, event_: &Event) {
        let Some(last) = self.last else {
            return;
        };

        let mut scope = ControlScope {
            widget: last,
            events: &mut self.events,
            drag_files: None,
        };

        let widget = &mut self.widgets[scope.widget];
        let mut event = event_.scope(widget.rect);
        event.kind = EventKind::MouseLeave;
        widget.inner.handle_event(&mut scope, event);
        self.last = None;
    }

    fn drag_enter(&mut self, files: &mut Vec<PathBuf>) -> bool {
        self.drag_files = Some(core::mem::take(files));
        true
    }

    fn handle_event(
        &mut self,
        event_: Event,
    ) -> bool {
        let x = event_.x;
        let y = event_.y;
        let mut target = self.test_widgets(x, y);

        if target.is_none()
            && self.capture_mouse.is_none()
            && self.last.is_none()
        {
            return false;
        }

        if self.last != target {
            self.mouse_leave(&event_);

            if let Some(i) = target {
                let mut scope = ControlScope {
                    widget: i,
                    events: &mut self.events,
                    drag_files: self.drag_files.as_ref().map(|v| &**v),
                };

                let widget = &mut self.widgets[scope.widget];
                let mut event = event_.scope(widget.rect);
                event.kind = EventKind::MouseEnter(event_.kind.is_dragdrop());
                widget.inner.handle_event(&mut scope, event);
                self.last = target;
            }
        }

        if let Some(force) = event_.breaks_capture()
            && (force || self.capture_mouse != target)
        {
            self.lost_focus();
        }

        target = self.capture_mouse.or(target);

        if let Some(i) = target {
            let mut scope = ControlScope {
                widget: i,
                events: &mut self.events,
                drag_files: self.drag_files.as_ref().map(|v| &**v),
            };

            let widget = &mut self.widgets[scope.widget];
            let mut event = event_.scope(widget.rect);

            if event.kind == EventKind::MouseLeftPress && widget.config.listen_double_click {
                let current = Instant::now();
                if let Some((clicked_i, time, cx, cy)) = self.clicked {
                    let delta = current.duration_since(time);
                    if clicked_i == i
                        && delta < self.dbl_click_msec
                        && (x - cx).abs() < self.dbl_click_width
                        && (y - cy).abs() < self.dbl_click_height
                    {
                        event.kind = EventKind::MouseDoubleClick;
                        self.clicked = None;
                    } else {
                        self.clicked = Some((i, current, x, y));
                    }
                } else {
                    self.clicked = Some((i, current, x, y));
                }
            }

            widget.inner.handle_event(&mut scope, event);
        }

        self.handle_events();

        target.is_some()
    }

    pub fn render(&mut self, draw: &mut DrawScope) {
        for widget in &mut self.widgets {
            if widget.visible {
                draw.set_translation(widget.rect[0] as f32, widget.rect[1] as f32);
                widget.inner.render(draw);
            }
        }
        draw.set_translation(0.0, 0.0);

        self.dirty = false;
    }

    fn handle_events(&mut self) {
        let mut events = core::mem::take(&mut self.events);
        let mut capture = None;
        let mut redraw = false;
        let mut post_events = Vec::new();
        for event in events.drain(..) {
            match event {
                WidgetEvent::Toggle(widget) => {
                    let widget = &mut self.widgets[widget];
                    widget.visible = !widget.visible;
                    redraw = true;
                }
                WidgetEvent::Hide(target) => {
                    let widget = &mut self.widgets[target];
                    if widget.visible {
                        widget.visible = false;
                        redraw = true;
                        post_events.push((target, EventKind::Hide));
                    }
                }
                WidgetEvent::Show(target) => {
                    let widget = &mut self.widgets[target];
                    if !widget.visible {
                        widget.visible = true;
                        redraw = true;
                        post_events.push((target, EventKind::Show));
                    }
                }
                WidgetEvent::Move(client, widget, x, y) => {
                    let client = &self.widgets[client];
                    let x0 = x + client.rect[0] as i32;
                    let y0 = y + client.rect[1] as i32;

                    let widget = &mut self.widgets[widget];
                    let x1 = x0 + (widget.rect[2] - widget.rect[0]) as i32;
                    let y1 = y0 + (widget.rect[3] - widget.rect[1]) as i32;
                    if x0 >= 0 && y0 >= 0 {
                        widget.rect = [
                            x0 as u32,
                            y0 as u32,
                            x1 as u32,
                            y1 as u32,
                        ];
                    }
                }
                WidgetEvent::Resize(widget, width, height) => {
                    let widget = &mut self.widgets[widget];
                    widget.rect[2] = widget.rect[0] + width;
                    widget.rect[3] = widget.rect[1] + height;
                }
                WidgetEvent::CaptureMouse(capture_) => capture = Some(capture_),
                WidgetEvent::SendEvent(target, event) => post_events.push((target, EventKind::Custom(event))),
                WidgetEvent::Redraw => redraw = true,
            }
        }
        self.events = events;

        if let Some(capture) = capture
            && capture != self.capture_mouse
        {
            if let Some(old) = self.capture_mouse {
                post_events.push((old, EventKind::LostFocus));
            }
            self.capture_mouse = capture;
        }

        if !post_events.is_empty() {
            let mut scope = ControlScope {
                widget: 0,
                events: &mut self.events,
                drag_files: None,
            };

            let mut event = Event {
                kind: EventKind::LostFocus,
                ctrl: false,
                shift: false,
                x: -1,
                y: -1,
            };

            for (target, kind) in post_events {
                scope.widget = target;
                event.kind = kind;
                let widget = &mut self.widgets[scope.widget];
                widget.inner.handle_event(&mut scope, event.clone());
            }

            self.handle_events();
        }

        if redraw && !self.dirty {
            self.dirty = true;
            update_display(&self.display);
        }
    }

    fn lost_focus(&mut self) {
        let mut scope = ControlScope {
            widget: 0,
            events: &mut self.events,
            drag_files: None,
        };

        if let Some(i) = self.capture_mouse.take() {
            let widget = &mut self.widgets[i];
            scope.widget = i;
            let event = Event {
                kind: EventKind::LostFocus,
                ctrl: false,
                shift: false,
                x: -1,
                y: -1,
            };
            widget.inner.handle_event(&mut scope, event);
        }

        self.handle_events();
    }
}

pub struct ControlScope<'a> {
    widget: usize,
    events: &'a mut Vec<WidgetEvent>,
    drag_files: Option<&'a [PathBuf]>
}

impl<'a> ControlScope<'a> {
    pub fn drag_files(&self) -> Option<&[PathBuf]> {
        self.drag_files
    }

    pub fn capture_mouse(&mut self) {
        self.events.push(WidgetEvent::CaptureMouse(Some(self.widget)));
    }

    pub fn release_mouse(&mut self) {
        self.events.push(WidgetEvent::CaptureMouse(None));
    }

    pub fn move_widget(&mut self, widget: usize, x: i32, y: i32) {
        self.events.push(WidgetEvent::Move(self.widget, widget, x, y));
    }

    #[allow(dead_code)]
    pub fn resize_widget(&mut self, widget: usize, width: u32, height: u32) {
        self.events.push(WidgetEvent::Resize(widget, width, height));
    }

    pub fn toggle_widget(&mut self, widget: usize) {
        self.events.push(WidgetEvent::Toggle(widget));
    }

    pub fn hide_widget(&mut self, widget: usize) {
        self.events.push(WidgetEvent::Hide(widget));
    }

    pub fn show_widget(&mut self, widget: usize) {
        self.events.push(WidgetEvent::Show(widget));
    }

    pub fn send_event(&mut self, target: usize, event: u32) {
        self.events.push(WidgetEvent::SendEvent(target, event));
    }

    pub fn redraw(&mut self) {
        self.events.push(WidgetEvent::Redraw);
    }
}

pub static CONTROL: Mutex<Option<Control>> = Mutex::new(None);

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let res = crate::panic::leak_unwind(|| {
        let mut control_ = CONTROL.lock().unwrap();
        let control = control_.as_mut().unwrap();
        let hook = *control.hooks.iter()
            .find_map(|(check, hook)| (*check == hwnd).then_some(hook))
            .unwrap();

        let event = if msg == Control::WM_PRIV_MOUSE {
            Event::from_msg(&control.hwnd, l_param.0 as u32, w_param.0)
        } else if msg == Control::WM_PRIV_DRAGMOVE
            || msg == Control::WM_PRIV_DRAGDROP
        {
            assert!(core::mem::size_of::<LPARAM>() == core::mem::size_of::<u64>());
            let l = l_param.0 as u64;
            let mut y = (l >> 32) as i32;
            let mut x = l as i32;
            unsafe {
                let mut rect = RECT::default();
                if GetWindowRect(control.hwnd, &mut rect).is_ok() {
                    x -= rect.left;
                    y -= rect.top;
                    Some(Event {
                        kind: if msg == Control::WM_PRIV_DRAGMOVE {
                            EventKind::MouseMove(true)
                        } else {
                            EventKind::DragDrop
                        },
                        ctrl: false,
                        shift: false,
                        x,
                        y,
                    })
                } else {
                    None
                }
            }
        } else {
            Event::from_msg(&control.hwnd, msg, w_param.0)
        };

        if let Some(event) = event {
            if control.test_widgets(event.x, event.y).is_some() {
                if msg != Control::WM_PRIV_MOUSE {
                    control.handle_event(event);
                }

                if Event::can_capture(msg) {
                    return Ok(0);
                }
            } else if msg == Control::WM_PRIV_MOUSE {
                control.handle_event(event);
                return Ok(0);
            } else if Event::can_capture(msg) && control.capture_mouse.is_some() {
                return Ok(0);
            }
        } else if msg == Control::WM_PRIV_DRAGENTER {
            control.mouse_leave(&Default::default());
            let files = unsafe {
                assert!(w_param.0 != 0 && w_param.0 % 8 == 0);
                &mut *(w_param.0 as *mut Vec<PathBuf>)
            };
            control.drag_enter(files);
            return Ok(1);
        } else if msg == Control::WM_PRIV_MOUSELEAVE {
            control.mouse_leave(&Event {
                kind: EventKind::MouseLeave,
                ctrl: false,
                shift: false,
                x: -1,
                y: -1,
            });
            control.drag_files = None;
        } else if msg == WM_KILLFOCUS {
            control.lost_focus();
        } else if msg == WM_NCDESTROY {
            for (i, (check, _)) in control.hooks.iter().enumerate() {
                if *check == hwnd {
                    control.hooks.remove(i);
                    break;
                }
            }

            if control.hooks.is_empty() {
                *control_ = None;
                drop(control_);

                // we don't block on GlobalMouseHook creation so possible race
                let mut hook = MOUSE_HOOK.lock().unwrap();
                if let Some(hook) = hook.take() {
                    unsafe {
                        let _ = UnhookWindowsHookEx(hook.1);
                    }
                }
            }
        }

        if msg == Control::WM_PRIV_MOUSE {
            Ok(0)
        } else {
            Err(hook)
        }
    });

    match res {
        Some(Err(hook)) => unsafe {
            CallWindowProcW(Some(hook), hwnd, msg, w_param, l_param)
        },
        Some(Ok(res)) => LRESULT(res),
        _ => LRESULT(0),
    }
}

static MOUSE_HOOK: Mutex<Option<GlobalMouseHook>> = Mutex::new(None);

unsafe extern "system" fn mouse_ll_proc(
    code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    crate::panic::leak_unwind(|| {
        if code >= 0 {
            let msg = w_param.0 as u32;
            let mut hook_ = MOUSE_HOOK.lock().unwrap();
            if let Some(hook) = hook_.as_mut() {
                let thread_id = hook.0;
                drop(hook_);

                unsafe {
                    let hwnd = GetForegroundWindow();
                    let current_thread_id = GetWindowThreadProcessId(hwnd, None);
                    if current_thread_id == thread_id {
                        let res = PostMessageW(
                            Some(hwnd),
                            Control::WM_PRIV_MOUSE,
                            WPARAM(0),
                            LPARAM(msg as isize),
                        );
                        if let Err(err) = res {
                            eprintln!("failed PostMessageW: {err:?}");
                        }
                    }
                }
            }
        }
    });

    unsafe {
        CallNextHookEx(None, code, w_param, l_param)
    }
}

struct GlobalMouseHook(u32, HHOOK);
unsafe impl Send for GlobalMouseHook {}

impl GlobalMouseHook {
    fn start(hwnd: HWND) {
        let hwnd_ = hwnd.0 as isize;
        // TODO: should we use std::thread::spawn or CreateThread?
        std::thread::spawn(move || {
            let thread_id;
            let hhook;
            {
                let mut hook = MOUSE_HOOK.lock().unwrap();
                let hwnd = HWND(hwnd_ as _);
                unsafe {
                    thread_id = GetWindowThreadProcessId(hwnd, None);
                    hhook = SetWindowsHookExW(
                        WH_MOUSE_LL,
                        Some(mouse_ll_proc),
                        None,
                        0,
                    ).unwrap();
                }
                *hook = Some(GlobalMouseHook(thread_id, hhook));
            }

            let hhook = hhook.0 as usize;
            crate::panic::on_unwind(move || {
                unsafe {
                    let _ = UnhookWindowsHookEx(HHOOK(hhook as *mut _));
                }
            });

            let mut msg = MSG::default();
            unsafe {
                loop {
                    if GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        _ = TranslateMessage(&msg);
                        _ = DispatchMessageW(&msg);
                    } else {
                        break;
                    }
                }
            }
        });
    }
}

fn update_display(hwnd: &HWND) {
    unsafe {
        let _ = PostMessageW(
            Some(*hwnd),
            WM_SIZE,
            Default::default(),
            Default::default(),
        );
    }
}
