# SoundFix

SoundFix is a small Windows utility that fixes audio clicking issues on Windows 11 in multi-GPU multi-monitor setups with Nvidia graphics cards.

The fix may look strange, but the issue appears to be related to a DWM bug that can affect audio stability. Moving DWM into a different internal state avoids the problem.

## What It Does

- Creates a tiny invisible window.
- Starts capturing that window through WinRT `GraphicsCapture` API.
- Immediately drops captured frames.

CPU and GPU load should be minimal because frames are not processed. Expected footprint is about 50 MB of RAM and 20 MB of VRAM.

## Build

```powershell
cargo.exe build --release
```
