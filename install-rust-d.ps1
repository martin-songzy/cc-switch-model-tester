# ============================================================
#  Rust 工具链安装脚本（安装到 D 盘）
#  用途：为 cc-switch Model Tester 开发环境准备 Rust + C++ 编译器
#  用法：在 PowerShell 7 中执行  & "D:\Claude_Code\workspace\llm-api-test\install-rust-d.ps1"
# ============================================================

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$RustRoot    = 'D:\Program Files\Rust'
$RustupHome  = "$RustRoot\rustup"     # Rust 本体（工具链）
$CargoHome   = "$RustRoot\cargo"      # Cargo 及其缓存
$BuildToolsPath = 'D:\Program Files\BuildTools'   # 微软 C++ 编译工具

Write-Host '==============================================' -ForegroundColor Cyan
Write-Host ' 步骤 1/4：设置用户环境变量（Rust 安装到 D 盘的关键）' -ForegroundColor Cyan
Write-Host '==============================================' -ForegroundColor Cyan
[Environment]::SetEnvironmentVariable('RUSTUP_HOME', $RustupHome, 'User')
[Environment]::SetEnvironmentVariable('CARGO_HOME',  $CargoHome,  'User')
$env:RUSTUP_HOME = $RustupHome
$env:CARGO_HOME  = $CargoHome
# 把 cargo\bin 加进用户 PATH（幂等）
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$CargoHome\bin*") {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$CargoHome\bin", 'User')
    Write-Host "  已将 $CargoHome\bin 加入用户 PATH"
}
$env:Path += ";$CargoHome\bin"
New-Item -ItemType Directory -Force -Path $RustRoot | Out-Null
Write-Host "  RUSTUP_HOME = $RustupHome"
Write-Host "  CARGO_HOME  = $CargoHome"
Write-Host '  OK' -ForegroundColor Green

Write-Host ''
Write-Host '==============================================' -ForegroundColor Cyan
Write-Host ' 步骤 2/4：检查/安装微软 C++ 编译工具（Build Tools）' -ForegroundColor Cyan
Write-Host '==============================================' -ForegroundColor Cyan
$vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$hasMsvc = $false
if (Test-Path $vswhere) {
    $inst = & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($inst) { $hasMsvc = $true; Write-Host "  已检测到 C++ 编译工具：$inst，跳过安装" }
}
if (-not $hasMsvc) {
    Write-Host "  未检测到 C++ 编译工具，开始安装到 $BuildToolsPath ..."
    Write-Host '  （约 3~4 GB，下载安装需 10~30 分钟，中途可能弹出管理员确认框请点"是"）'
    winget install --id Microsoft.VisualStudio.2022.BuildTools `
        --accept-package-agreements --accept-source-agreements `
        --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --installPath `"$BuildToolsPath`""
    if ($LASTEXITCODE -ne 0) { throw "Build Tools 安装失败（exit=$LASTEXITCODE）。请把本提示截图反馈。" }
    Write-Host '  OK' -ForegroundColor Green
}

Write-Host ''
Write-Host '==============================================' -ForegroundColor Cyan
Write-Host ' 步骤 3/4：安装 Rust 本体（rustup，minimal 精简版）' -ForegroundColor Cyan
Write-Host '==============================================' -ForegroundColor Cyan
$rustcExe = "$CargoHome\bin\rustc.exe"
if (Test-Path $rustcExe) {
    Write-Host '  已检测到 rustc，跳过安装'
} else {
    $init = "$env:TEMP\rustup-init.exe"
    Write-Host '  下载 rustup-init.exe ...'
    Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile $init -UseBasicParsing
    Write-Host '  静默安装（minimal 精简组件，约 1GB）...'
    & $init -y --default-host x86_64-pc-windows-msvc --default-toolchain stable --profile minimal
    if ($LASTEXITCODE -ne 0) { throw "rustup 安装失败（exit=$LASTEXITCODE）。请把本提示截图反馈。" }
    Write-Host '  OK' -ForegroundColor Green
}

Write-Host ''
Write-Host '==============================================' -ForegroundColor Cyan
Write-Host ' 步骤 4/4：验证安装' -ForegroundColor Cyan
Write-Host '==============================================' -ForegroundColor Cyan
& "$CargoHome\bin\rustc.exe" --version
& "$CargoHome\bin\cargo.exe" --version

Write-Host ''
Write-Host '==============================================' -ForegroundColor Green
Write-Host ' 全部完成！请关闭本窗口，重新打开一个 PowerShell 7 窗口，' -ForegroundColor Green
Write-Host ' 输入 rustc --version 能显示版本号即表示环境变量生效。' -ForegroundColor Green
Write-Host '==============================================' -ForegroundColor Green
