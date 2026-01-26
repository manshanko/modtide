use crate::dxgi::SolidColorBrush;
use crate::dxgi::TextFormat;

use super::list::ModListEvent;
use super::list::ModListWidget;
use super::Control;
use super::ControlScope;
use super::Event;
use super::EventKind;

static MENU: &[&[(&str, ModListEvent)]] = &[
    &[
        ("Toggle", ModListEvent::ToggleSelected),
        ("Browse", ModListEvent::OpenSelected),
    ],
    &[
        ("Toggle Patch", ModListEvent::TogglePatch),
        ("Sort Mods", ModListEvent::SortMods),
        ("Browse Darktide", ModListEvent::BrowseDarktide),
        ("Browse Logs", ModListEvent::BrowseLogs),
    ],
];

pub enum DropdownMenu {
    ModSelected = 0,
    Meta = 1,
}

impl DropdownMenu {
    fn from_u32(msg: u32) -> Option<Self> {
        Some(match msg {
            0 => DropdownMenu::ModSelected,
            1 => DropdownMenu::Meta,
            _ => return None,
        })
    }
}

pub struct DropdownWidget {
    brush: SolidColorBrush,
    text_format: TextFormat,

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
        brush: SolidColorBrush,
        text_format: TextFormat,
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

    pub fn show(control: &mut ControlScope, x: i32, y: i32, menu: DropdownMenu) {
        control.send_event(Control::DROPDOWN_WIDGET, menu as u32);
        control.move_widget(Control::DROPDOWN_WIDGET, x, y);
        control.show_widget(Control::DROPDOWN_WIDGET);
    }

    pub fn hide(control: &mut ControlScope) {
        control.hide_widget(Control::DROPDOWN_WIDGET);
    }

    fn menu(&self) -> &[(&str, ModListEvent)] {
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
        control: &mut ControlScope,
        event: Event,
    ) {
        'control: {
            match event.kind {
                EventKind::Show => control.capture_mouse(),
                EventKind::Hide => {
                    self.hovered_option = None;
                    control.release_mouse();
                }
                EventKind::LostFocus => control.hide_widget(Control::DROPDOWN_WIDGET),
                EventKind::Custom(msg) => {
                    if let Some(menu) = DropdownMenu::from_u32(msg) {
                        self.menu = menu as usize;
                    }
                }
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
            EventKind::MouseMove(_) if !is_inside => {
                if self.hovered_option.is_some() {
                    self.hovered_option = None;
                    control.redraw();
                }
            }
            EventKind::MouseMove(_) => {
                let offset = y - Self::BORDER_SIZE as i32;
                let opt = offset / Self::ENTRY_HEIGHT as i32;

                let new_opt = if opt < 0 || opt >= menu.len() as i32 {
                    None
                } else {
                    Some(opt as usize)
                };

                if self.hovered_option != new_opt {
                    self.hovered_option = new_opt;
                    control.redraw();
                }
            }

            EventKind::MouseLeftRelease
            | EventKind::MouseRightRelease if is_inside => {
                let offset = y as u32 - Self::BORDER_SIZE;
                let opt = (offset / Self::ENTRY_HEIGHT) as usize;
                if let Some((_, event)) = menu.get(opt) {
                    ModListWidget::send(control, event.clone());
                }
                DropdownWidget::hide(control);
            }

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

        self.brush.set_color(&Self::BACKGROUND);
        context.fill_rounded_rect(
            &self.brush,
            rect,
            radius,
        );

        self.brush.set_color(&Self::BORDER);
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
                self.brush.set_color(&Self::HIGHLIGHT);

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

            self.brush.set_color(&Self::TEXT_COLOR);

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
