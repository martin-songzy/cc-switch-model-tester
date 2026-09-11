@echo off
rem ============================================================
rem  一键构建正式版 cc-switch Model Tester
rem  双击运行，或在任意命令行中执行：%~dp0build.cmd
rem  产物：dist\cc-switch-model-tester.exe
rem ============================================================

rem --- 修复 VS BuildTools 实例注册丢失导致的工具链定位失败：
rem --- 前置 MSVC/SDK 工具目录，确保 cl.exe / link.exe / lib.exe / rc.exe 可被找到
set "PATH=D:\Program Files\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64;C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64;%PATH%"

rem --- Rust 工具链（安装于 D:\Program Files\Rust）
set "RUSTUP_HOME=D:\Program Files\Rust\rustup"
set "CARGO_HOME=D:\Program Files\Rust\cargo"
set "PATH=D:\Program Files\Rust\cargo\bin;%PATH%"

cd /d "%~dp0src-tauri"
rem 注意：必须走 Tauri CLI（npm run tauri build），它会注入界面打包模式；
rem 直接 cargo build --release 生成的 exe 会去找 localhost 开发服务器（黑屏报错）。
cd /d "%~dp0"
npm run tauri build -- --no-bundle
if errorlevel 1 (
  echo.
  echo [错误] 构建失败，请把上方错误信息反馈给开发者。
  pause
  exit /b 1
)

if not exist "%~dp0dist" mkdir "%~dp0dist"
copy /y "%~dp0src-tauri\target\release\cc-switch-model-tester.exe" "%~dp0dist\cc-switch-model-tester.exe" >nul
echo.
echo [完成] 构建成功：%~dp0dist\cc-switch-model-tester.exe
pause
