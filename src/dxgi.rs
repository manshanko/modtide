// TODO: custom font loading with IDWriteInMemoryFontFileLoader for OTF (OTTO)
use std::os::windows::ffi::OsStrExt;
use std::ffi::OsStr;

use windows::core::PCWSTR;
use windows::core::Result;
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;
use windows::Win32::System::Com::CoCreateInstance;
use windows::Win32::UI::Shell::SHCreateMemStream;

const FEATURE_LEVELS: &[D3D_FEATURE_LEVEL] = &[
    D3D_FEATURE_LEVEL_11_1,
    D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_9_3,
    D3D_FEATURE_LEVEL_9_2,
    D3D_FEATURE_LEVEL_9_1,
];

pub struct DxgiContext {
    factory: ID2D1Factory1,
    dwfactory: IDWriteFactory,
    device: ID3D11Device,
    context: ID2D1RenderTarget,
    d2dcontext: ID2D1DeviceContext,

    width: u32,
    height: u32,
}

#[allow(dead_code)]
impl DxgiContext {
    //const DEFAULT_WIDTH: u32 = 1280;
    //const DEFAULT_HEIGHT: u32 = 940;
    const DEFAULT_WIDTH: u32 = 1;
    const DEFAULT_HEIGHT: u32 = 1;

    fn create_texture2d_(
        device: &ID3D11Device,
        width: u32,
        height: u32,
    ) -> Result<ID3D11Texture2D> {
        unsafe {
            let mut props: D3D11_TEXTURE2D_DESC = core::mem::zeroed();
            props.ArraySize = 1;
            props.BindFlags = D3D11_BIND_RENDER_TARGET.0 as u32;
            props.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
            props.Width = width;
            props.Height = height;
            props.MipLevels = 1;
            props.SampleDesc.Count = 1;
            props.MiscFlags = D3D11_RESOURCE_MISC_GDI_COMPATIBLE.0 as u32;

            let mut texture = None;
            device.CreateTexture2D(&props, None, Some(&mut texture))?;
            Ok(texture.unwrap())
        }
    }

    fn resize_(
        factory: &ID2D1Factory1,
        device: &ID3D11Device,
        width: u32,
        height: u32,
    ) -> Result<ID2D1RenderTarget> {
        unsafe {
            let texture = Self::create_texture2d_(device, width, height)?;
            let surface = texture.cast::<IDXGISurface>()?;

            let mut props: D2D1_RENDER_TARGET_PROPERTIES = core::mem::zeroed();
            props.pixelFormat.format = DXGI_FORMAT_B8G8R8A8_UNORM;
            props.pixelFormat.alphaMode = D2D1_ALPHA_MODE_PREMULTIPLIED;
            props.dpiX = 0.0;
            props.dpiY = 0.0;
            props.usage = D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE;

            factory.CreateDxgiSurfaceRenderTarget(&surface, &props)
        }
    }

    pub fn new() -> Result<Self> {
        let factory: ID2D1Factory1;
        let dwfactory;
        let device;
        let context;
        let d2dcontext;
        unsafe {
            let mut device_ = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE(core::ptr::null_mut()),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(FEATURE_LEVELS),
                D3D11_SDK_VERSION,
                Some(&mut device_),
                None,
                None,
            )?;
            device = device_.unwrap();

            factory = D2D1CreateFactory(
                D2D1_FACTORY_TYPE_MULTI_THREADED,
                None,
            )?;

            dwfactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let dxgi = device.cast::<IDXGIDevice1>()?;
            let d2d = factory.CreateDevice(&dxgi)?;
            d2dcontext = d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

            context = Self::resize_(&factory, &device, Self::DEFAULT_WIDTH, Self::DEFAULT_HEIGHT)?;
        }

        Ok(Self {
            factory,
            dwfactory,
            device,
            context,
            d2dcontext,

            width: Self::DEFAULT_WIDTH,
            height: Self::DEFAULT_HEIGHT,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool> {
        if width != self.width || height != self.height {
            self.context = Self::resize_(
                &self.factory,
                &self.device,
                width,
                height,
            )?;
            self.width = width;
            self.height = height;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn create_texture2d(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<ID3D11Texture2D> {
        Self::create_texture2d_(&self.device, width, height)
    }

    pub fn create_solid_color_brush(
        &mut self,
        color: &[f32; 4],
    ) -> Result<ID2D1SolidColorBrush> {
        let color = D2D1_COLOR_F {
            r: color[0],
            g: color[1],
            b: color[2],
            a: color[3],
        };
        unsafe {
            self.context.CreateSolidColorBrush(
                &color,
                None,
            )
        }
    }

    pub fn create_bitmap(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<ID2D1Bitmap> {
        unsafe {
            let mut desc: D3D11_TEXTURE2D_DESC = core::mem::zeroed();
            desc.Width = width;
            desc.Height = height;
            desc.MipLevels = 1;
            desc.ArraySize = 1;
            desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
            desc.MiscFlags = D3D11_RESOURCE_MISC_SHARED.0 as u32;
            desc.BindFlags = (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET).0 as u32;

            let mut texture = None;
            self.device.CreateTexture2D(
                &desc,
                None,
                Some(&mut texture),
            )?;
            let texture = texture.unwrap();

            let surface = texture.cast::<IDXGISurface>().unwrap();
            let bitmap = self.d2dcontext.CreateBitmapFromDxgiSurface(&surface, None)?;
            Ok(bitmap.into())
        }
    }

    pub fn create_bitmap_from_png(
        &mut self,
        png: &[u8],
        callback: Option<fn(&mut [[u8; 4]])>,
    ) -> Result<ID2D1Bitmap> {
        unsafe {
            let stream = SHCreateMemStream(Some(png)).unwrap();

            let decoder: IWICBitmapDecoder = CoCreateInstance(
                &CLSID_WICPngDecoder,
                None,
                CLSCTX_INPROC_SERVER,
            )?;
            decoder.Initialize(&stream, WICDecodeMetadataCacheOnLoad)?;

            let frame = decoder.GetFrame(0)?;
            let format = frame.GetPixelFormat()?;
            let bitmap = if format == GUID_WICPixelFormat32bppPBGRA {
                frame.into()
            } else {
                WICConvertBitmapSource(&GUID_WICPixelFormat32bppPBGRA, &frame)?
            };

            if let Some(callback) = callback {
                let factory: IWICImagingFactory = CoCreateInstance(
                    &CLSID_WICImagingFactory,
                    None,
                    CLSCTX_INPROC_SERVER,
                )?;

                let bitmap = factory.CreateBitmapFromSource(&bitmap, WICBitmapCacheOnDemand)?;

                let mut width = 0;
                let mut height = 0;
                bitmap.GetSize(&mut width, &mut height)?;
                let rect = WICRect {
                    X: 0,
                    Y: 0,
                    Width: width as i32,
                    Height: height as i32,
                };
                let lock = bitmap.Lock(&rect, (WICBitmapLockRead.0 | WICBitmapLockWrite.0) as u32)?;
                let mut len = 0;
                let mut ptr = core::ptr::null_mut();
                lock.GetDataPointer(&mut len, &mut ptr)?;
                callback(core::slice::from_raw_parts_mut(ptr as *mut _, (len / 4) as usize));

                drop(lock);
                self.context.CreateBitmapFromWicBitmap(&bitmap, None)
            } else {
                self.context.CreateBitmapFromWicBitmap(&bitmap, None)
            }
        }
    }

    pub fn create_bitmap_from_texture2d(
        &mut self,
        texture: &ID3D11Texture2D,
    ) -> Result<ID2D1Bitmap> {
        let surface = texture.cast::<IDXGISurface>()?;
        unsafe {
            self.d2dcontext.CreateBitmapFromDxgiSurface(&surface, None)
                .map(|b| b.into())
        }
    }

    pub fn create_text_format(
        &mut self,
        font_family: PCWSTR,
        font_size: f32,
    ) -> Result<IDWriteTextFormat> {
        unsafe {
            self.dwfactory.CreateTextFormat(
                font_family,
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size,
                windows::core::w!("en-us"),
            )
        }
    }

    pub fn create_text_layout(
        &mut self,
        text: &[u16],
        text_format: &IDWriteTextFormat,
        width: f32,
        height: f32,
    ) -> Result<IDWriteTextLayout> {
        unsafe {
            self.dwfactory.CreateTextLayout(
                text,
                text_format,
                width,
                height,
            )
        }
    }

    pub fn create_compatible_render_target(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<DrawScope<'_>> {
        unsafe {
            let size = D2D_SIZE_U {
                width,
                height,
            };
            let context = self.context.CreateCompatibleRenderTarget(
                None,
                Some(&size),
                None,
                D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
            )?;

            context.BeginDraw();

            Ok(DrawScope {
                context: context.into(),
                _marker: Default::default(),
            })
        }
    }

    pub fn begin_draw(&self) -> DrawScope<'_> {
        unsafe {
            self.context.BeginDraw();
        }
        DrawScope {
            context: self.context.clone(),
            _marker: Default::default(),
        }
    }
}

pub struct DrawScope<'a> {
    context: ID2D1RenderTarget,
    _marker: core::marker::PhantomData<&'a ()>,
}

impl<'a> DrawScope<'a> {
    pub fn clear(&mut self) {
        unsafe {
            self.context.Clear(None);
        }
    }

    pub fn set_translation(
        &mut self,
        x: f32,
        y: f32,
    ) {
        let mat: [f32; 6] = [
            1.0,
            0.0,
            0.0,
            1.0,
            x,
            y,
        ];
        unsafe {
            self.context.SetTransform(mat.as_ptr() as *const _);
        }
    }

    //pub fn draw_texture(
    //    &mut self,
    //    texture: &ID3D11Texture2D,
    //) -> Result<()> {
    //    let surface = texture.cast::<IDXGISurface>()?;
    //    unsafe {
    //        let bitmap = self.0.d2dcontext.CreateBitmapFromDxgiSurface(&surface, None)?;
    //        self.context.DrawBitmap(
    //            &bitmap,
    //            None,
    //            1.0,
    //            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    //            None,
    //        );
    //    }
    //
    //    Ok(())
    //}

    //pub fn draw_surface9(
    //    &mut self,
    //    surface9: &IDirect3DSurface9,
    //) -> Result<()> {
    //    let res = surface9.cast::<IDXGIResource>()?;
    //    unsafe {
    //        let dxgi = self.0.dxgi.CreateSurfaceFromHandle(res)?;
    //        let mut bitmap = None;
    //        self.context.CreateSharedBitmap(&ID2D1Bitmap::IID, &dxgi, None, &mut bitmap)?;
    //        let bitmap = bitmap.unwrap();
    //        self.context.DrawBitmap(
    //            &bitmap,
    //            None,
    //            1.0,
    //            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    //            None,
    //        );
    //    }
    //
    //    Ok(())
    //}

    pub fn draw_bitmap(
        &mut self,
        bitmap: &ID2D1Bitmap,
        dest: Option<&[f32; 4]>,
        src: Option<&[f32; 4]>,
    ) {
        unsafe {
            self.context.DrawBitmap(
                bitmap,
                dest.map(|a| a as *const _ as *const _),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                src.map(|a| a as *const _ as *const _),
            );
        }
    }

    pub fn draw_line(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        brush: &ID2D1SolidColorBrush,
        size: f32,
    ) {
        unsafe {
            self.context.DrawLine(
                core::mem::transmute(from),
                core::mem::transmute(to),
                brush,
                size,
                None,
            )
        }
    }

    pub fn draw_text(
        &mut self,
        text: &OsStr,
        text_format: &IDWriteTextFormat,
        brush: &ID2D1SolidColorBrush,
        rect: &[f32; 4],
    ) {
        let _owner;
        let mut buf = [0; 256];
        let text = if text.len() > buf.len() {
            let mut b = Vec::new();
            for c in text.encode_wide() {
                b.push(c);
            }
            _owner = b;
            &_owner
        } else {
            let mut i = 0;
            for c in text.encode_wide() {
                buf[i] = c;
                i += 1;
            }
            &buf[0..i]
        };

        let rect = D2D_RECT_F {
            left: rect[0],
            top: rect[1],
            right: rect[2],
            bottom: rect[3],
        };
        unsafe {
            self.context.DrawText(
                text,
                text_format,
                &rect,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    pub fn draw_rounded_rect(
        &mut self,
        brush: &ID2D1SolidColorBrush,
        rect: [f32; 4],
        radius: f32,
        size: f32,
    ) {
        unsafe {
            let round = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: rect[0],
                    top: rect[1],
                    right: rect[2],
                    bottom: rect[3],
                },
                radiusX: radius,
                radiusY: radius,
            };
            self.context.DrawRoundedRectangle(
                &round,
                brush,
                size,
                None,
            )
        }
    }

    pub fn fill_rounded_rect(
        &mut self,
        brush: &ID2D1SolidColorBrush,
        rect: [f32; 4],
        radius: f32,
    ) {
        unsafe {
            let round = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: rect[0],
                    top: rect[1],
                    right: rect[2],
                    bottom: rect[3],
                },
                radiusX: radius,
                radiusY: radius,
            };
            self.context.FillRoundedRectangle(
                &round,
                brush,
            )
        }
    }

    pub fn push_axis_aligned_clip(
        &mut self,
        rect: &[f32; 4],
    ) {
        let rect = D2D_RECT_F {
            left: rect[0],
            top: rect[1],
            right: rect[2],
            bottom: rect[3],
        };
        unsafe {
            self.context.PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    pub fn pop_axis_aligned_clip(
        &mut self,
    ) {
        unsafe {
            self.context.PopAxisAlignedClip();
        }
    }

    pub fn get_dc(&mut self) -> Result<HdcScope<'_>> {
        let (interop, hdc) = unsafe {
            let interop: ID2D1GdiInteropRenderTarget = self.context.cast()?;
            let hdc = interop.GetDC(D2D1_DC_INITIALIZE_MODE_COPY)?;
            (interop, hdc)
        };
        Ok(HdcScope {
            hdc,
            interop,
            _marker: Default::default(),
        })
    }

    pub fn get_bitmap(&mut self) -> Result<ID2D1Bitmap> {
        unsafe {
            let context: ID2D1BitmapRenderTarget = self.context.cast()?;
            context.GetBitmap()
        }
    }
}

impl<'a> Drop for DrawScope<'a> {
    fn drop(&mut self) {
        unsafe {
            let _ = self.context.EndDraw(None, None);
        }
    }
}

pub struct HdcScope<'a> {
    hdc: HDC,
    interop: ID2D1GdiInteropRenderTarget,
    _marker: core::marker::PhantomData<&'a ()>,
}

impl<'a> HdcScope<'a> {
    pub fn hdc(&self) -> HDC {
        self.hdc
    }
}

impl<'a> Drop for HdcScope<'a> {
    fn drop(&mut self) {
        unsafe {
            let _ = self.interop.ReleaseDC(None);
        }
    }
}
