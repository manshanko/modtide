use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::dxgi::DrawScope;

pub mod button;
pub mod list;

pub trait Widget: Send + 'static {
    fn config(&self) -> WidgetConfig {
        Default::default()
    }

    fn rect(&self, width: u32, height: u32) -> [u32; 4];

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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EventKind {
    MouseMove,
    MousePress,
    MouseRelease,
    MouseDoubleClick,
    MouseScroll(i32),
    MouseEnter,
    MouseLeave,
    KeyDown(KeyKind),
    //LostFocus,
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
            WM_SETCURSOR => EventKind::MouseMove,
            WM_MOUSEMOVE => EventKind::MouseMove,
            WM_LBUTTONDOWN => EventKind::MousePress,
            WM_LBUTTONUP => EventKind::MouseRelease,
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
                    _ => return None,
                };
                EventKind::KeyDown(kind)
            }
            _ => return None,
        };

        let mut ctrl = false;
        let mut shift = false;
        if kind == EventKind::MousePress {
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

    fn scope(&self, rect: [u32; 4]) -> Self {
        let mut out = self.clone();
        out.x -= rect[0] as i32;
        out.y -= rect[1] as i32;
        out
    }
}

enum WidgetEvent {
    Toggle(usize),
    CaptureMouse(Option<usize>),
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
    display: HWND,
    capture_mouse: Option<usize>,
    last: Option<usize>,
    widgets: Vec<WidgetState>,
    events: Vec<WidgetEvent>,

    dirty: bool,

    clicked: Option<(usize, Instant, i32, i32)>,
    dbl_click_msec: Duration,
    dbl_click_width: i32,
    dbl_click_height: i32,

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

    const WM_PRIV_MOUSE: u32 = WM_APP + 0x333;

    pub fn hook(
        button: button::ButtonWidget,
        mod_list: list::ModListWidget,
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

        for widget in &mut widgets {
            widget.rect = widget.inner.rect(width, height);
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
            display: display.unwrap_or(hwnd),
            capture_mouse: None,
            last: None,
            widgets,
            events: Vec::new(),

            dirty: false,

            clicked: None,
            dbl_click_msec,
            dbl_click_width,
            dbl_click_height,

            hooks,
        });

        GlobalMouseHook::start(hwnd);
    }

    fn test_widgets(&self, x: i32, y: i32) -> Option<usize> {
        let x = u32::try_from(x).ok()?;
        let y = u32::try_from(y).ok()?;

        for (i, widget) in self.widgets.iter().enumerate() {
            if !widget.visible {
                continue;
            }

            let x0 = widget.rect[0];
            let y0 = widget.rect[1];
            let x1 = widget.rect[2];
            let y1 = widget.rect[3];
            if x >= x0 && x < x1
                && y >= y0 && y < y1
            {
                return Some(i);
            }
        }

        None
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

        let mut scope = ControlScope {
            widget: 0,
            events: &mut self.events,
        };

        if self.last != target {
            if let Some(last) = self.last {
                scope.widget = last;
                let widget = &mut self.widgets[scope.widget];
                let mut event = event_.scope(widget.rect);
                event.kind = EventKind::MouseLeave;
                widget.inner.handle_event(&mut scope, event);
                self.last = None;
            }

            if let Some(i) = target {
                scope.widget = i;
                let widget = &mut self.widgets[scope.widget];
                let mut event = event_.scope(widget.rect);
                event.kind = EventKind::MouseEnter;
                widget.inner.handle_event(&mut scope, event);
                self.last = target;
            }
        }

        target = self.capture_mouse.or(target);

        if let Some(i) = target {
            scope.widget = i;
            let widget = &mut self.widgets[scope.widget];
            let mut event = event_.scope(widget.rect);

            if event.kind == EventKind::MousePress && widget.config.listen_double_click {
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
        for event in events.drain(..) {
            match event {
                WidgetEvent::Toggle(widget) => {
                    let widget = &mut self.widgets[widget];
                    widget.visible = !widget.visible;
                }
                WidgetEvent::CaptureMouse(capture_) => capture = Some(capture_),
                WidgetEvent::Redraw => {
                    if !self.dirty {
                        self.dirty = true;
                        update_display(&self.display);
                    }
                }
            }
        }
        self.events = events;

        if let Some(capture) = capture
            && capture != self.capture_mouse
        {
            self.capture_mouse = capture;
        }
    }

    //pub fn lost_focus(&mut self) {
    //    let mut scope = ControlScope {
    //        widget: 0,
    //        events: &mut self.events,
    //    };
    //
    //    for (i, widget) in self.widgets.iter_mut().enumerate() {
    //        scope.widget = i;
    //        let event = Event {
    //            kind: EventKind::LostFocus,
    //            ctrl: false,
    //            shift: false,
    //            x: -1,
    //            y: -1,
    //        };
    //        widget.inner.handle_event(&mut scope, event);
    //    }
    //
    //    self.handle_events();
    //}
}

pub struct ControlScope<'a> {
    widget: usize,
    events: &'a mut Vec<WidgetEvent>,
}

impl<'a> ControlScope<'a> {
    pub fn capture_mouse(&mut self) {
        self.events.push(WidgetEvent::CaptureMouse(Some(self.widget)));
    }

    pub fn release_mouse(&mut self) {
        self.events.push(WidgetEvent::CaptureMouse(None));
    }

    #[allow(dead_code)]
    pub fn toggle_self(&mut self) {
        self.toggle_widget(self.widget);
    }

    pub fn toggle_widget(&mut self, widget: usize) {
        self.events.push(WidgetEvent::Toggle(widget));
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
        } else {
            Event::from_msg(&control.hwnd, msg, w_param.0)
        };

        if let Some(event) = event {
            if control.test_widgets(event.x, event.y).is_some() {
                if msg != Control::WM_PRIV_MOUSE {
                    control.handle_event(event);
                }

                if Event::can_capture(msg) {
                    return None;
                }
            } else if msg == Control::WM_PRIV_MOUSE {
                control.handle_event(event);
                return None;
            } else if Event::can_capture(msg) && control.capture_mouse.is_some() {
                return None;
            }
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
            return None;
        }

        Some(hook)
    });

    if let Some(Some(hook)) = res {
        unsafe {
            CallWindowProcW(Some(hook), hwnd, msg, w_param, l_param)
        }
    } else {
        LRESULT(0)
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

#[allow(dead_code)]
pub fn log(s: &str) {
    use std::io::Write;

    let mut fd = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open("dt-mod-manager-log.txt")
        .unwrap();
    writeln!(&mut fd, "{s}").unwrap();
}
