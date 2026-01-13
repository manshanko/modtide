use windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use super::Control;
use super::Event;
use super::EventKind;
use super::KeyKind;
use super::CustomEvent;

static MENU: &[&[(&str, CustomEvent)]] = &[
    &[("Browse", CustomEvent::Open)]
];

pub struct DropdownWidget {
    brush: ID2D1SolidColorBrush,
    text_format: IDWriteTextFormat,

    width: u32,
    height: u32,

    hovered_option: Option<usize>,
    menu: usize,
}

impl DropdownWidget {
    const BORDER_SIZE: u32 = 2;
    const PADDING_Y: u32 = 2;
    const ENTRY_HEIGHT: u32 = 26;

    const BACKGROUND: [f32; 4] = [0.05, 0.05, 0.05, 1.0];
    const BORDER: [f32; 4] = [0.6, 0.6, 0.6, 1.0];
    const TEXT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    const HIGHLIGHT: [f32; 4] = [0.15, 0.15, 0.15, 1.0];

    pub fn new(
        brush: ID2D1SolidColorBrush,
        text_format: IDWriteTextFormat,
    ) -> Self {
        Self {
            brush,
            text_format,

            width: 180,
            height: 400,

            hovered_option: None,
            menu: 0,
        }
    }

    fn menu(&self) -> &[(&str, CustomEvent)] {
        MENU.get(self.menu).cloned().unwrap_or(&[])
    }
}

impl super::Widget for DropdownWidget {
    fn rect(&self, _width: u32, _height: u32) -> [u32; 4] {
        [
            0,
            0,
            self.width,
            self.height,
        ]
    }

    fn hit_test(&self, _x: u32, y: u32) -> bool {
        let padding = (Self::BORDER_SIZE + Self::PADDING_Y) * 2;
        y < padding * 2 + Self::ENTRY_HEIGHT * self.menu().len() as u32
    }

    fn handle_event(
        &mut self,
        control: &mut super::ControlScope,
        event: Event,
    ) {
        'control: {
            match event.kind {
                EventKind::Show => control.capture_mouse(),
                EventKind::Hide => control.release_mouse(),
                EventKind::LostFocus => control.hide_widget(Control::DROPDOWN_WIDGET),
                _ => break 'control,
            }
            return;
        }

        let x = event.x;
        let y = event.y;
        let menu = self.menu();
        let padding = (Self::BORDER_SIZE + Self::PADDING_Y) * 2;
        let is_inside = y >= 0 && (y as u32) < padding + Self::ENTRY_HEIGHT * menu.len() as u32
            && x >= 0 && x < self.width as i32;

        match event.kind {
            EventKind::MouseMove => {
                let offset = y as u32 - Self::BORDER_SIZE;
                let opt = (offset / Self::ENTRY_HEIGHT) as usize;
                if self.hovered_option != Some(opt) {
                    self.hovered_option = Some(opt);
                    control.redraw();
                }
            }

            EventKind::MouseLeftRelease
            | EventKind::MouseRightRelease if is_inside => {
                let offset = y as u32 - Self::BORDER_SIZE;
                let opt = (offset / Self::ENTRY_HEIGHT) as usize;
                if let Some((_, event)) = menu.get(opt) {
                    control.send_event(Control::MOD_LIST_WIDGET, *event);
                }
                control.hide_widget(Control::DROPDOWN_WIDGET);
            }

            EventKind::MouseLeftPress
            | EventKind::MouseRightPress
            | EventKind::KeyDown(KeyKind::Escape)
                if !is_inside => control.hide_widget(Control::DROPDOWN_WIDGET),

            _ => (),
        }
    }

    fn render(&mut self, context: &mut super::DrawScope) {
        let menu = self.menu();

        let padding = (Self::BORDER_SIZE + Self::PADDING_Y) as f32;
        let border = Self::BORDER_SIZE as f32 / 2.0;
        let rect = [
            border,
            border,
            self.width as f32 - border,
            (menu.len() * Self::ENTRY_HEIGHT as usize) as f32 + padding * 2.0 - border,
        ];
        let radius = 2.0;

        unsafe {
            self.brush.SetColor(Self::BACKGROUND.as_ptr() as *const _);
        }
        context.fill_rounded_rect(
            &self.brush,
            rect,
            radius,
        );

        unsafe {
            self.brush.SetColor(Self::BORDER.as_ptr() as *const _);
        }
        context.draw_rounded_rect(
            &self.brush,
            rect,
            radius,
            2.0,
        );

        let mut o = padding;
        for (i, (text, _)) in menu.iter().enumerate() {
            let rectf = [
                (Self::BORDER_SIZE + 4) as f32,
                o,
                (self.width - Self::BORDER_SIZE - 4) as f32,
                o + Self::ENTRY_HEIGHT as f32,
            ];

            if Some(i) == self.hovered_option {
                unsafe {
                    self.brush.SetColor(Self::HIGHLIGHT.as_ptr() as *const _);
                }

                let mid = o + Self::ENTRY_HEIGHT as f32 / 2.0;
                let from = [
                    4.0,
                    mid,
                ];
                let to = [
                    (self.width - 4) as f32,
                    mid,
                ];
                context.draw_line(from, to, &self.brush, (Self::ENTRY_HEIGHT - 4) as f32);
            }

            unsafe {
                self.brush.SetColor(Self::TEXT_COLOR.as_ptr() as *const _);
            }

            context.draw_text(
                text.as_ref(),
                &self.text_format,
                &self.brush,
                &rectf,
            );

            o += Self::ENTRY_HEIGHT as f32;
        }
    }
}
