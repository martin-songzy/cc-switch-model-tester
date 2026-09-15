# cleanup-build-env.ps1 —— 移除本地 Rust / MSVC 编译环境（编译已全部转移到 GitHub Actions 云端）
# 释放空间：约 4.8 GB（Rust 工具链 1.42 GB + VS BuildTools 3.35 GB）
#
# 卸载方式：
#   - Rust：rustup 无安装器，官方推荐卸载法 = 删目录 + 清环境变量（本脚本做法，无残留）
#   - BuildTools：先调用微软官方卸载器静默卸载（最干净），完成后兜底删除残留目录
#   - 脚本需要管理员权限（BuildTools 官方卸载要求），会自动弹出 UAC 提权确认
#
# 说明：
#   1. 不影响已构建的 exe（dist\cc-switch-model-tester-0.1.1.exe 照常可用）
#   2. node / node_modules（前端依赖与 svelte-check）保留
#   3. C:\Program Files (x86)\Microsoft Visual Studio\Installer（VS 卸载器本体，约几十 MB）保留，
#      以便将来还能正规卸载其他 VS 产品；不需要的话可手动删除
#   4. 可重复运行

$ErrorActionPreference = 'Stop'

# ---------- 0. 管理员权限检查（BuildTools 官方卸载需要） ----------
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host '需要管理员权限（BuildTools 官方卸载要求），正在弹出 UAC 确认...' -ForegroundColor Yellow
    Start-Process -FilePath 'pwsh' -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`"" -Verb RunAs
    exit
}

# ---------- 待删清单 ----------
$rustDir       = 'D:\Program Files\Rust'        # rustup + cargo + 工具链
$buildToolsDir = 'D:\Program Files\BuildTools'  # MSVC 链接器 + Windows SDK
$vsSetupExe    = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe'
$envVars       = @('RUSTUP_HOME', 'CARGO_HOME') # 用户级环境变量

function Get-DirGB($p) {
    if (Test-Path $p) {
        (Get-ChildItem $p -Recurse -Force -ErrorAction SilentlyContinue |
         Measure-Object -Property Length -Sum).Sum / 1GB
    } else { $null }
}

# ---------- 1. 统计并展示 ----------
$total = 0
$rustGB = Get-DirGB $rustDir
$btGB   = Get-DirGB $buildToolsDir
if ($rustGB) { '{0,-40} {1,8:N2} GB' -f $rustDir, $rustGB; $total += $rustGB }
else         { '{0,-40} 不存在（跳过）' -f $rustDir }
if ($btGB)   { '{0,-40} {1,8:N2} GB' -f $buildToolsDir, $btGB; $total += $btGB }
else         { '{0,-40} 不存在（跳过）' -f $buildToolsDir }

if (-not $rustGB -and -not $btGB) { Write-Host '没有需要删除的内容，脚本结束。'; exit 0 }

Write-Host ''
Write-Host ("合计约 {0:N2} GB。方式：Rust=删目录+清环境变量；BuildTools=官方卸载器+兜底删残留。" -f $total)
$answer = Read-Host '确认执行？输入 Y 继续，其他任意键取消'
if ($answer -ne 'Y' -and $answer -ne 'y') { Write-Host '已取消，未做任何改动。'; exit 0 }

# ---------- 2. Rust：清环境变量 + 删目录 ----------
foreach ($v in $envVars) {
    [Environment]::SetEnvironmentVariable($v, $null, 'User')
    Write-Host "已清除用户级环境变量 $v"
}
if ($rustGB) {
    try {
        Remove-Item $rustDir -Recurse -Force -ErrorAction Stop
        Write-Host "已删除 $rustDir"
    } catch {
        Write-Host "删除 $rustDir 失败：$($_.Exception.Message)" -ForegroundColor Red
        Write-Host '请关闭占用该目录的程序后重试。' -ForegroundColor Yellow
        exit 1
    }
}

# ---------- 3. BuildTools：官方静默卸载 ----------
if ($btGB) {
    if (Test-Path $vsSetupExe) {
        Write-Host '调用微软官方卸载器静默卸载 BuildTools（约 2-6 分钟，请勿关机）...'
        $proc = Start-Process -FilePath $vsSetupExe `
            -ArgumentList 'uninstall', '--installPath', "`"$buildToolsDir`"", '--quiet', '--norestart' `
            -PassThru -Wait
        Write-Host ("卸载器退出码: {0}" -f $proc.ExitCode)
        # 轮询等待目录消失（最长 10 分钟）
        $deadline = (Get-Date).AddMinutes(10)
        while ((Test-Path $buildToolsDir) -and (Get-Date) -lt $deadline) {
            Start-Sleep -Seconds 15
        }
    } else {
        Write-Host '未找到官方卸载器，将直接删除目录。' -ForegroundColor Yellow
    }
    # 兜底：官方卸载后残留（或未找到卸载器）→ 直接删
    if (Test-Path $buildToolsDir) {
        Write-Host '清理官方卸载后的残留目录...'
        try {
            Remove-Item $buildToolsDir -Recurse -Force -ErrorAction Stop
            Write-Host "已删除 $buildToolsDir"
        } catch {
            Write-Host "删除残留失败：$($_.Exception.Message)" -ForegroundColor Red
            Write-Host '可稍后重试；或打开「设置-应用」找到 Visual Studio Build Tools 点卸载。' -ForegroundColor Yellow
            exit 1
        }
    } else {
        Write-Host 'BuildTools 已由官方卸载器彻底移除。'
    }
}

# ---------- 4. 完成报告 ----------
Write-Host ''
Write-Host ("完成：本地编译环境已移除，释放约 {0:N2} GB。" -f $total)
Write-Host '提示：VS 卸载器本体（C:\Program Files (x86)\Microsoft Visual Studio\Installer）已保留，'
Write-Host '      如需彻底移除可手动删除该目录（不影响系统）。'
