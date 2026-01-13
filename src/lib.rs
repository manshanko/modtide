use core::ffi::c_void;
use std::io::Read;
use std::path::Path;

use windows::Win32::Foundation::GetLastError;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

mod extract;
mod hook;
mod dxgi;
mod panic;
mod widget;
use widget::button::ButtonWidget;
use widget::dropdown::DropdownWidget;
use widget::list::ModListWidget;
mod mod_engine;

// TODO: stub like wine/dlls/dwmapi/dwmapi_main.c
#[unsafe(no_mangle)]
extern "system" fn DwmapiNoImpl() -> u32 {
    0x80263001
}

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(
    _hinst_dll: *const (),
    reason: u64,
    _reserved: *const (),
) -> u32 {
    if reason == 1 {
        unsafe {
            let _ = windows::Win32::System::Threading::CreateThread(
                None,
                4 * 1024 * 1024,
                Some(init_),
                None,
                windows::Win32::System::Threading::THREAD_CREATION_FLAGS(0),
                None,
            );
        }
    }

    1
}

unsafe extern "system" fn init_(_: *mut c_void) -> u32 {
    panic::leak_unwind(|| {
        let _ = init();
    });
    0
}

const LAUNCHER: &str = "launcher\\launcher.exe";
const LAUNCHER2: &str = "launcher\\Launcher.exe";
const RESOURCE_DICTIONARY: &str = "launcher\\ResourceDictionary.dll";

fn init() -> Result<(), Box<dyn std::error::Error>> {
    panic::init();

    let Ok(file_path) = std::env::current_exe() else {
        return Ok(());
    };
    if !(file_path.ends_with(Path::new(LAUNCHER)) || file_path.ends_with(Path::new(LAUNCHER2))) {
        return Ok(());
    }

    let Some(root) = file_path.parent().and_then(Path::parent) else {
        eprintln!("failed to get root Darktide path");
        return Ok(());
    };

    let resource = root.join(RESOURCE_DICTIONARY);
    let mut resource = std::fs::File::open(resource)?;
    let mut data = Vec::new();
    resource.read_to_end(&mut data)?;

    let mut button_active = None;
    let mut button_idle = None;
    let mut background = None;
    for png in extract::ExtractPng::new(&data) {
        if let Some(file_name) = png.file_name {
            match file_name {
                "button_small_active.png" => button_active = Some(png.buffer),
                "button_small_idle.png" => button_idle = Some(png.buffer),
                "settings_background.png" => background = Some(png.buffer),
                _ => (),
            }
        }
    }

    let mut context = dxgi::DxgiContext::new().unwrap();
    let brush_color = [1.0, 1.0, 1.0, 1.0];
    let brush = context.create_solid_color_brush(&brush_color).unwrap();
    let text_format = context.create_text_format(windows::core::w!("Arial"), 17.0).unwrap();

    let (button_active, button_idle) = match (button_active, button_idle) {
        (Some(button_active), Some(button_idle)) => {
            (
                context.create_bitmap_from_png(button_active, None).unwrap(),
                context.create_bitmap_from_png(button_idle, None).unwrap(),
            )
        }
        _ => {
            let mut button_active = None;
            let mut button_idle = None;
            for (button, is_active) in [
                (&mut button_active, true),
                (&mut button_idle, false),
            ] {
                let mut draw = context.create_compatible_render_target(
                    ButtonWidget::WIDTH,
                    ButtonWidget::HEIGHT,
                ).unwrap();
                ButtonWidget::fallback(&mut draw, &brush, is_active);
                *button = draw.get_bitmap().ok();
            }

            (
                button_active.unwrap(),
                button_idle.unwrap(),
            )
        }
    };

    let background = if let Some(background) = background {
        context.create_bitmap_from_png(background, Some(reduce_alpha)).unwrap()
    } else {
        let mut draw = context.create_compatible_render_target(
            ModListWidget::WIDTH,
            ModListWidget::HEIGHT,
        ).unwrap();
        ModListWidget::fallback(&mut draw, &brush);
        draw.get_bitmap().unwrap()
    };

    unsafe {
        brush.SetColor(brush_color.as_ptr() as *const _);

        let size = button_active.GetPixelSize();
        let sizef = button_active.GetSize();
        let rectf = [
            0.0,
            0.0,
            sizef.width,
            sizef.height,
        ];

        text_format.SetTextAlignment(
            windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER).unwrap();
        text_format.SetParagraphAlignment(
            windows::Win32::Graphics::DirectWrite::DWRITE_PARAGRAPH_ALIGNMENT_CENTER).unwrap();
        //let text_layout = context.create_text_layout(
        //    &b"MODS".map(u16::from),
        //    &text_format,
        //    sizef.width,
        //    sizef.height,
        //).unwrap();

        let mut draw = context.create_compatible_render_target(size.width, size.height).unwrap();
        for bitmap in [&button_active, &button_idle] {
            draw.clear();
            draw.draw_bitmap(
                bitmap,
                None,
                None,
            );
            draw.draw_text(
                "MODS".as_ref(),
                &text_format,
                &brush,
                &rectf,
            );
            let target = draw.get_bitmap().unwrap();
            bitmap.CopyFromBitmap(None, &target, None).unwrap();
        }
        drop(draw);

        text_format.SetTextAlignment(
            windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_LEADING).unwrap();
        //text_format.SetParagraphAlignment(
        //    windows::Win32::Graphics::DirectWrite::DWRITE_PARAGRAPH_ALIGNMENT_CENTER).unwrap();
    }

    let dropdown = DropdownWidget::new(brush.clone(), text_format.clone());
    let button = ButtonWidget::new(button_active, button_idle);
    let mut mod_list = ModListWidget::new(
        root.join("mods"),
        background,
        brush,
        text_format);
    if let Err(err) = mod_list.mount() {
        eprintln!("failed mod list mount: {err:?}");
    }
    let mut widgets = Some((mod_list, button, dropdown));

    hook::hook_ulw(Box::new(move |hwnd, org_info| {
        // TODO: blur and dim widgets when settings are open
        if let Some(control) = &mut *widget::CONTROL.lock().unwrap()
            && hwnd != control.display // !control.is_hooked_hwnd(hwnd)
        {
            hook::update_layered_window_indirect(hwnd, org_info);
            return;
        }

        let mut rect;
        unsafe {
            rect = core::mem::zeroed();
            GetWindowRect(hwnd, &mut rect).unwrap();
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let widthu = u32::try_from(width).unwrap();
        let heightu = u32::try_from(height).unwrap();
        context.resize(widthu, heightu).unwrap();

        let bf = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        unsafe {
            let mut draw = context.begin_draw();
            draw.clear();
            if let Ok(hdc) = draw.get_dc() {
                let hdc = hdc.hdc();
                windows::Win32::Graphics::Gdi::BitBlt(
                    hdc,
                    0,
                    0,
                    width,
                    height,
                    Some(org_info.hdcSrc),
                    0,
                    0,
                    SRCCOPY,
                ).unwrap();
            } else {
                eprintln!("failed to get DC: {:?}", GetLastError());
            }

            if let Some(control) = &mut *widget::CONTROL.lock().unwrap() {
                control.render(&mut draw);
            }

            if let Ok(hdc) = draw.get_dc() {
                let hdc = hdc.hdc();

                let mut info = *org_info;
                info.hdcSrc = hdc;
                info.pblend = &bf;
                info.pptDst = core::ptr::null();
                info.prcDirty = core::ptr::null();
                let res = hook::update_layered_window_indirect(hwnd, &info);
                if res == 0 {
                    eprintln!("error with UpdateLayeredWindow: {:?}", GetLastError());
                }
            } else {
                eprintln!("failed to get DC: {:?}", GetLastError());
            }
        }

        if let Some(w) = widgets.take() {
            widget::Control::hook(w.0, w.1, w.2, hwnd);
        }
    })).unwrap();

    Ok(())
}

fn reduce_alpha(buf: &mut [[u8; 4]]) {
    for pixel in buf {
        let mut p = *pixel;
        let a = p[3] as f32 / 255.0;
        if a > 0.5 && a < 1.0 {
            let diff = a.sqrt() / a;
            for b in &mut p {
                let new = (*b as f32 * diff).min(255.0);
                *b = new as u8;
            }
        }
        *pixel = p;
    }
}
