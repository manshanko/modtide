use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use windows::core::BOOL;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::System::Memory::VirtualProtect;
use windows::Win32::System::Memory::PAGE_EXECUTE_READWRITE;
use windows::Win32::UI::WindowsAndMessaging::UPDATELAYEREDWINDOWINFO;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Gdi::BLENDFUNCTION;

// link attribute from:
// https://github.com/microsoft/windows-rs/blob/9f0cf126f392f9e9d955f64703fd779d78cc345c/crates/libs/link/src/lib.rs
#[link(name = "user32.dll", kind = "raw-dylib", modifiers = "+verbatim")]
unsafe extern "system" {
    fn UpdateLayeredWindowIndirect(
        hwnd: HWND,
        info: *const UPDATELAYEREDWINDOWINFO,
    ) -> BOOL;
}

#[link(name = "win32u.dll", kind = "raw-dylib", modifiers = "+verbatim")]
unsafe extern "system" {
    // wine/dlls/win32u/window.c
    fn NtUserUpdateLayeredWindow(
        hwnd: HWND,
        hdcDst: HDC,
        pptDst: *const POINT,
        psize: *const SIZE,
        hdcSrc: HDC,
        pptSrc: *const POINT,
        crKey: COLORREF,
        pblend: *const BLENDFUNCTION,
        dwFlags: u32,
        prcDirty: *const RECT,
    ) -> BOOL;
}

type Callback = dyn FnMut(
    HWND,
    &UPDATELAYEREDWINDOWINFO,
) + Send;

static CALLBACK: Mutex<Option<Box<Callback>>> = Mutex::new(None);
static BYPASS: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn update_layered_window_indirect_hook(
    hwnd: HWND,
    info: *const UPDATELAYEREDWINDOWINFO,
) -> i32 {
    unsafe {
        if !BYPASS.load(Ordering::SeqCst)
            && let Ok(mut callback) = CALLBACK.lock()
            && callback.is_some()
        {
            let res = crate::panic::leak_unwind(move || {
                if !info.is_null() && let Some(callback) = &mut *callback {
                    callback(
                        hwnd,
                        &*info,
                    );
                }
            });

            if res.is_some() {
                0x77777777
            } else {
                let mut info = *info;
                info.prcDirty = core::ptr::null();
                update_layered_window_indirect(hwnd, &info)
            }
        } else {
            update_layered_window_indirect(hwnd, &*info)
        }
    }
}

pub fn update_layered_window_indirect(
    hwnd: HWND,
    info: &UPDATELAYEREDWINDOWINFO,
) -> i32 {
    unsafe {
        NtUserUpdateLayeredWindow(
            hwnd,
            info.hdcDst,
            info.pptDst,
            info.psize,
            info.hdcSrc,
            info.pptSrc,
            info.crKey,
            info.pblend,
            info.dwFlags.0,
            info.prcDirty,
        ).0
    }
}

pub fn hook_ulw(
    hook: Box<Callback>,
) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        {
            *CALLBACK.lock().unwrap() = Some(hook);
        }
        crate::panic::on_unwind(|| {
            BYPASS.store(true, Ordering::SeqCst);
        });

        let ptr = UpdateLayeredWindowIndirect as *mut u8;
        let mut old_flags = core::mem::zeroed();
        VirtualProtect(
            ptr as *const _,
            1024,
            PAGE_EXECUTE_READWRITE,
            &mut old_flags,
        )?;

        if cfg!(all(windows, target_arch = "x86_64")) {
            let addr = usize::to_ne_bytes(update_layered_window_indirect_hook as *const () as usize);
            let mut buf = [0xcc; 12];
            buf[0..2].copy_from_slice(&[0x48, 0xb8]);
            buf[2..10].copy_from_slice(&addr);
            buf[10..12].copy_from_slice(&[0xff, 0xe0]);
            core::ptr::copy(buf.as_ptr(), ptr, 12);
        } else {
            panic!("only windows x64 is supported");
        }

        VirtualProtect(
            ptr as *const _,
            1024,
            old_flags,
            &mut old_flags,
        )?;
    }

    Ok(())
}
