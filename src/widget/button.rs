use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use crate::dxgi::SolidColorBrush;

use super::Event;
use super::EventKind;

// launcher exit button is anchor
pub(super) const EXIT_WIDTH: u32 = 38;
pub(super) const EXIT_HEIGHT: u32 = 38;
pub(super) const EXIT_X_OFFSET: u32 = 26;
pub(super) const EXIT_Y_OFFSET: u32 = 77;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Idle,
    Hover,
    Held,
    Active,
}

pub struct ButtonWidget {
    active: ID2D1Bitmap,
    idle: ID2D1Bitmap,
    width: u32,
    height: u32,

    mode: Mode,
}

impl ButtonWidget {
    pub const WIDTH: u32 = 140;
    pub const HEIGHT: u32 = 48;

    pub(super) const MARGIN_RIGHT: u32 = EXIT_WIDTH + EXIT_X_OFFSET * 2;
    pub(super) const MARGIN_TOP: u32 = EXIT_Y_OFFSET + EXIT_HEIGHT / 2;

    const FALLBACK_ACTIVE: [f32; 4] = [0.2, 0.2, 0.2, 0.8];
    const FALLBACK_IDLE: [f32; 4] = [0.0, 0.0, 0.0, 0.8];
    const FALLBACK_BORDER: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    pub fn new(
        active: ID2D1Bitmap,
        idle: ID2D1Bitmap,
    ) -> Self {
        let size = unsafe { active.GetPixelSize() };
        Self {
            active,
            idle,
            width: size.width,
            height: size.height,

            mode: Mode::Idle,
        }
    }

    pub fn fallback(
        context: &mut super::DrawScope,
        brush: &SolidColorBrush,
        is_active: bool,
    ) {
        let rect = [
            4.0,
            4.0,
            (Self::WIDTH - 4) as f32,
            (Self::HEIGHT - 4) as f32,
        ];
        let radius = 2.0;

        let color = if is_active {
            Self::FALLBACK_ACTIVE
        } else {
            Self::FALLBACK_IDLE
        };
        brush.set_color(&color);
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
}

impl super::Widget for ButtonWidget {
    fn rect(&self, width: u32, _height: u32) -> [u32; 4] {
        [
            width - Self::MARGIN_RIGHT - self.width,
            Self::MARGIN_TOP - self.height / 2,
            width - Self::MARGIN_RIGHT,
            Self::MARGIN_TOP + self.height / 2,
        ]
    }

    fn handle_event(
        &mut self,
        control: &mut super::ControlScope,
        event: Event,
    ) {
        let x = event.x;
        let y = event.y;
        let intersect = x >= 0 && x < self.width as i32
            && y >= 0 && y < self.height as i32;

        let old = self.mode;
        match (event.kind, self.mode, intersect) {
            (EventKind::MouseEnter(_), Mode::Held  , _) => self.mode = Mode::Active,
            (EventKind::MouseEnter(_), _           , _) => self.mode = Mode::Hover,
            (EventKind::MouseLeave   , Mode::Active, _) => self.mode = Mode::Held,
            (EventKind::MouseLeave   , _           , _) => self.mode = Mode::Idle,

            (EventKind::MouseLeftRelease, _, true ) => self.mode = Mode::Hover,
            (EventKind::MouseLeftRelease, _, false) => self.mode = Mode::Idle,
            (EventKind::MouseLeftPress  , _, true ) => self.mode = Mode::Active,
            (EventKind::MouseLeftPress  , _, false) => self.mode = Mode::Idle,

            _ => (),
        }

        if old != self.mode {
            match event.kind {
                EventKind::MouseLeftRelease => {
                    control.release_mouse();
                    if old == Mode::Active {
                        control.toggle_widget(super::Control::MOD_LIST_WIDGET);
                    }
                }
                EventKind::MouseLeftPress => control.capture_mouse(),
                _ => (),
            }

            control.redraw();
        }
    }

    fn render(&mut self, context: &mut super::DrawScope) {
        let mut rect = [0.0, 0.0, self.width as f32, self.height as f32];
        if let Mode::Active = self.mode {
            let x = self.width as f32 * 0.03;
            let y = self.height as f32 * 0.03;
            rect[0] += x;
            rect[1] += y;
            rect[2] -= x;
            rect[3] -= y;
        }

        let bitmap = match self.mode {
            Mode::Idle => &self.idle,

            Mode::Held
            | Mode::Hover
            | Mode::Active => &self.active,
        };

        context.draw_bitmap(bitmap, Some(&rect), None);
    }
}
