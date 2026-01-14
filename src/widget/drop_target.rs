#![allow(unused_variables)]
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::os::windows::ffi::OsStringExt;

use windows::core::Ref;
use windows::core::Result;
use windows::core::implement;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::Control;

#[implement(IDropTarget)]
pub struct DropTarget {
    hwnd: HWND,
    valid_source: AtomicBool,
}

impl DropTarget {
    pub fn start(hwnd: HWND, display: HWND) {
        let hwnd_ = hwnd.0 as usize;
        let display_ = display.0 as usize;
        unsafe {
            std::thread::spawn(move || {
                let hwnd = HWND(hwnd_ as *mut _);
                let display = HWND(display_ as *mut _);
                let drop = Self {
                    hwnd,
                    valid_source: AtomicBool::new(false),
                };

                let _ = RevokeDragDrop(display);
                OleInitialize(None).unwrap();
                if let Err(err) = RegisterDragDrop(display, &IDropTarget::from(drop)) {
                    crate::log::log(&format!("{err:?}"));
                } else {
                    crate::panic::on_unwind(move || {
                        let _ = RevokeDragDrop(HWND(display_ as *mut _));
                    });

                    let mut msg = MSG::default();
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
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        _key_state: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            tymed: TYMED_HGLOBAL.0 as u32,
            dwAspect: DVASPECT_CONTENT.0,
            ..Default::default()
        };
        unsafe {
            *effect = DROPEFFECT_NONE;
            crate::panic::leak_unwind(|| {
                if let Ok(med) = data.unwrap().GetData(&format) {
                    let hdrop = HDROP(med.u.hGlobal.0);
                    let count = DragQueryFileW(
                        hdrop,
                        u32::MAX,
                        None,
                    );
                    if count == 0 {
                        return;
                    }

                    let mut buf = vec![0; 4097];

                    let mut out = Vec::new();
                    for i in 0..count {
                        let len = DragQueryFileW(
                            hdrop,
                            i,
                            Some(&mut buf),
                        );
                        let path = &buf[0..len as usize];
                        out.push(PathBuf::from(OsString::from_wide(&path)));
                    }

                    let res = SendMessageW(
                        self.this.hwnd,
                        Control::WM_PRIV_DRAGENTER,
                        Some(WPARAM(&mut out as *mut _ as usize)),
                        Default::default(),
                    );

                    let is_valid = res == LRESULT(1);
                    self.valid_source.store(is_valid, Ordering::SeqCst);
                    if is_valid {
                        *effect = DROPEFFECT_COPY;
                    } else {
                        *effect = DROPEFFECT_NONE;
                    }
                }
            });
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _key_state: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        unsafe {
            if self.valid_source.load(Ordering::SeqCst) {
                *effect = DROPEFFECT_COPY;
                crate::panic::leak_unwind(|| {
                    assert!(core::mem::size_of::<POINTL>() == core::mem::size_of::<LPARAM>());
                    let _ = PostMessageW(
                        Some(self.this.hwnd),
                        Control::WM_PRIV_DRAGMOVE,
                        Default::default(),
                        *(pt as *const _ as *const _),
                    );
                });
            } else {
                *effect = DROPEFFECT_NONE;
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        unsafe {
            if self.valid_source.load(Ordering::SeqCst) {
                let _ = PostMessageW(
                    Some(self.this.hwnd),
                    Control::WM_PRIV_MOUSELEAVE,
                    Default::default(),
                    Default::default(),
                );
            }
        }
        Ok(())
    }

    fn Drop(
        &self,
        _data: Ref<'_, IDataObject>,
        _key_state: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        unsafe {
            if self.valid_source.load(Ordering::SeqCst) {
                *effect = DROPEFFECT_COPY;
                let _ = PostMessageW(
                    Some(self.this.hwnd),
                    Control::WM_PRIV_DRAGDROP,
                    Default::default(),
                    *(pt as *const _ as *const _),
                );
            } else {
                *effect = DROPEFFECT_NONE;
            }
        }
        Ok(())
    }
}
