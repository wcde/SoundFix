#![cfg_attr(not(test), windows_subsystem = "windows")]

use std::{
    mem::size_of,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use windows::{
    Foundation::TypedEventHandler,
    Graphics::{Capture::*, DirectX::Direct3D11::*, DirectX::*},
    Win32::{
        Foundation::*,
        Graphics::{Direct3D::*, Direct3D11::*, Dxgi::*},
        System::{
            LibraryLoader::GetModuleHandleW,
            WinRT::{
                Direct3D11::CreateDirect3D11DeviceFromDXGIDevice,
                Graphics::Capture::IGraphicsCaptureItemInterop,
            },
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::*,
};

const TRAY_ICON_ID: u32 = 1;
const TRAY_CALLBACK_MESSAGE: u32 = WM_USER + 1;
const CLOSE_COMMAND_ID: usize = 1;
const CAPTURE_WINDOW_SIZE: i32 = 1;

fn create_d3d_device() -> Result<IDirect3DDevice> {
    unsafe {
        let mut d3d_device: Option<ID3D11Device> = None;
        let mut d3d_context: Option<ID3D11DeviceContext> = None;

        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[
                D3D_FEATURE_LEVEL_11_1,
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_10_0,
            ]),
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            None,
            Some(&mut d3d_context),
        )?;

        let d3d_device = d3d_device.unwrap();
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;

        let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)?;
        let direct3d_device: IDirect3DDevice = inspectable.cast()?;

        Ok(direct3d_device)
    }
}

fn create_capture_window() -> Result<HWND> {
    unsafe {
        let class_name = w!("SoundFixCaptureWindow");
        let instance = GetModuleHandleW(None)?;
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(capture_window_proc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        if RegisterClassW(&window_class) == 0 {
            return Err(Error::from_win32());
        }

        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("SoundFix capture"),
            WS_POPUP,
            0,
            0,
            CAPTURE_WINDOW_SIZE,
            CAPTURE_WINDOW_SIZE,
            None,
            None,
            instance,
            None,
        )
        .inspect(|window| {
            let _ = ShowWindow(*window, SW_SHOWNOACTIVATE);
        })
    }
}

unsafe extern "system" fn capture_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn create_capture_item_for_window(window: HWND) -> Result<GraphicsCaptureItem> {
    unsafe {
        let factory: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;

        let item: GraphicsCaptureItem = factory.CreateForWindow(window)?;

        Ok(item)
    }
}

fn run_capture(stop: Receiver<()>) -> Result<()> {
    let device = create_d3d_device()?;
    let window = create_capture_window()?;
    let item = create_capture_item_for_window(window)?;
    hide_capture_window_from_task_switcher(window);
    let size = item.Size()?;

    let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        size,
    )?;

    let session = frame_pool.CreateCaptureSession(&item)?;
    let pool_for_callback = frame_pool.clone();

    let _frame_arrived_token = frame_pool.FrameArrived(&TypedEventHandler::<
        Direct3D11CaptureFramePool,
        IInspectable,
    >::new(move |_, _| {
        while let Ok(frame) = pool_for_callback.TryGetNextFrame() {
            drop(frame);
        }

        Ok(())
    }))?;

    session.SetIsBorderRequired(false)?;
    session.StartCapture()?;

    loop {
        if stop.recv_timeout(Duration::from_secs(1)).is_ok() {
            unsafe { DestroyWindow(window)? };
            return Ok(());
        }
    }
}

fn hide_capture_window_from_task_switcher(window: HWND) {
    unsafe {
        let extended_style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
        let _ = SetWindowLongPtrW(
            window,
            GWL_EXSTYLE,
            (extended_style | WS_EX_TOOLWINDOW.0) as isize,
        );
    }
}

fn run_tray() -> Result<()> {
    let (stop_tx, stop_rx) = mpsc::channel();
    let capture_thread = thread::spawn(move || run_capture(stop_rx));
    let tray_result = tray_message_loop();

    let _ = stop_tx.send(());
    match capture_thread.join() {
        Ok(capture_result) => capture_result?,
        Err(_) => return Err(Error::new(E_FAIL, "Capture thread panicked")),
    }

    tray_result
}

fn tray_message_loop() -> Result<()> {
    unsafe {
        let class_name = w!("SoundFixTrayWindow");
        let instance = GetModuleHandleW(None)?;
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(tray_window_proc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        if RegisterClassW(&window_class) == 0 {
            return Err(Error::from_win32());
        }

        let window = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("SoundFix"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        )?;

        let icon = LoadIconW(None, IDI_APPLICATION)?;
        let mut icon_data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: TRAY_ICON_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK_MESSAGE,
            hIcon: icon,
            ..Default::default()
        };
        copy_wide_text(&mut icon_data.szTip, "SoundFix");

        if !Shell_NotifyIconW(NIM_ADD, &icon_data).as_bool() {
            DestroyWindow(window)?;
            return Err(Error::from_win32());
        }

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data);
        DestroyWindow(window)?;
        Ok(())
    }
}

fn copy_wide_text(buffer: &mut [u16], text: &str) {
    for (slot, value) in buffer.iter_mut().zip(text.encode_utf16()) {
        *slot = value;
    }
}

unsafe extern "system" fn tray_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        TRAY_CALLBACK_MESSAGE if lparam.0 as u32 == WM_RBUTTONUP => {
            let _ = unsafe { show_tray_menu(window) };
            LRESULT::default()
        }
        WM_COMMAND if wparam.0 & 0xffff == CLOSE_COMMAND_ID => {
            unsafe { PostQuitMessage(0) };
            LRESULT::default()
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT::default()
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe fn show_tray_menu(window: HWND) -> Result<()> {
    let menu = unsafe { CreatePopupMenu()? };
    unsafe { AppendMenuW(menu, MF_STRING, CLOSE_COMMAND_ID, w!("Close"))? };

    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor)? };
    let _ = unsafe { SetForegroundWindow(window) };
    let _ = unsafe {
        TrackPopupMenu(
            menu,
            TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            0,
            window,
            None,
        )
    };
    unsafe { DestroyMenu(menu)? };
    Ok(())
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    run_tray()?;
    Ok(())
}
