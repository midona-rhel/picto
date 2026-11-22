# TEMPORARY. Takes pictures of glass panels in the real app on Windows, which
# the DirectX renderer draws, and pictures of the same panels in the
# off-screen tool, which the wgpu renderer draws. The job `verify:app` of
# `.github/workflows/verify.yml` runs this script and uploads the pictures.
param(
    # The app's program.
    [Parameter(Mandatory)] [string] $App,
    # The test program, which holds the off-screen tool.
    [Parameter(Mandatory)] [string] $Tests,
    # A disposable copy of the test library.
    [Parameter(Mandatory)] [string] $Copy,
    # The folder that the pictures and the logs are written to.
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type -Namespace Win -Name User32 -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out RECT rect);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref POINT point);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
'@
[Win.User32]::SetProcessDPIAware() | Out-Null

New-Item -ItemType Directory -Force -Path $Out | Out-Null

# The runner's screen is 1024 by 768 pixels unless it is changed. The window
# of the off-screen tool is 1213 by 865 points, so the screen is made larger.
try { Set-DisplayResolution -Width 1920 -Height 1080 -Force | Out-Null } catch { "Set-DisplayResolution failed: $_" }
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
"screen: $($screen.Width) x $($screen.Height)"
"build: $((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuild)"
"adapter: $((Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }) -join ', ')"
$size = if ($screen.Width -ge 1300 -and $screen.Height -ge 950) { '1213x865' } else { '1000x700' }
"window: $size"

# Runs the off-screen tool on the copy with the script lines $lines, and
# saves its pictures in the folder $folder.
function OffScreen($lines, $folder) {
    $script = Join-Path $Out 'script.txt'
    Set-Content -Path $script -Value $lines -Encoding ascii
    $env:PICTO_GPUI_COPY = $Copy
    $env:PICTO_GPUI_SCRIPT = $script
    $env:PICTO_GPUI_OUT = $folder
    $env:PICTO_GPUI_MENU = 'window'
    & $Tests --ignored --exact offscreen::plays_a_script --nocapture | Select-Object -Last 4
    if ($LASTEXITCODE -ne 0) { throw "the off-screen tool failed with $LASTEXITCODE" }
    Remove-Item Env:PICTO_GPUI_COPY, Env:PICTO_GPUI_SCRIPT, Env:PICTO_GPUI_OUT, Env:PICTO_GPUI_MENU
}

# Saves a picture of the client area of $window as $name.png.
function Shot($window, $name) {
    $rect = New-Object Win.User32+RECT
    [Win.User32]::GetClientRect($window, [ref] $rect) | Out-Null
    $origin = New-Object Win.User32+POINT
    [Win.User32]::ClientToScreen($window, [ref] $origin) | Out-Null
    $bitmap = New-Object System.Drawing.Bitmap $rect.Right, $rect.Bottom
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($origin.X, $origin.Y, 0, 0, $bitmap.Size)
    $bitmap.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()
    "picture: $name ($($rect.Right) x $($rect.Bottom) at $($origin.X), $($origin.Y))"
}

# Presses and releases the right mouse button at the point ($x, $y) of the
# client area of $window.
function RightClick($window, $x, $y) {
    $origin = New-Object Win.User32+POINT
    [Win.User32]::ClientToScreen($window, [ref] $origin) | Out-Null
    [Win.User32]::SetCursorPos($origin.X + $x, $origin.Y + $y) | Out-Null
    Start-Sleep -Milliseconds 500
    [Win.User32]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    [Win.User32]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
}

$env:PICTO_GPUI_HOLD_RUNS = '1'
$env:PICTO_GPUI_HOLD_DOWNLOADS = '1'
$env:PICTO_GPUI_SILENT = '1'

$env:PICTO_GPUI_WINDOW = $size

foreach ($theme in 'dark', 'light') {
    # Stores the theme in the copy. The off-screen tool is then started
    # again for the pictures, so that it starts in that theme.
    OffScreen @(
        "invoke settings.patch {`"value`":{`"colorScheme`":`"$theme`"}}",
        'wait 500'
    ) (Join-Path $Out 'patch')
    OffScreen @(
        'wait 1500',
        'shot grid',
        'move 450 150',
        'rdown 450 150',
        'rup 450 150',
        'wait 800',
        'shot item-menu',
        'key escape',
        'wait 400',
        'key secondary-t',
        'wait 800',
        'shot quick-search'
    ) (Join-Path $Out "offscreen-$theme")

    $env:PICTO_GPUI_STATE = Join-Path $Out "state-$theme"
    $env:PICTO_GPUI_APP_DATA = Join-Path $Out "data-$theme"
    New-Item -ItemType Directory -Force -Path $env:PICTO_GPUI_STATE, $env:PICTO_GPUI_APP_DATA | Out-Null
    $process = Start-Process -FilePath $App -ArgumentList "`"$Copy`"" -PassThru `
        -RedirectStandardOutput (Join-Path $Out "app-$theme.out.log") `
        -RedirectStandardError (Join-Path $Out "app-$theme.err.log")
    $window = [IntPtr]::Zero
    for ($second = 0; $second -lt 120 -and $window -eq [IntPtr]::Zero; $second++) {
        Start-Sleep 1
        $process.Refresh()
        if ($process.HasExited) { throw "the app ended before its window opened ($theme)" }
        $window = $process.MainWindowHandle
    }
    if ($window -eq [IntPtr]::Zero) { throw "the app opened no window in 120 seconds ($theme)" }
    # The library needs a moment to open and to draw its thumbnails.
    Start-Sleep 12
    [Win.User32]::SetForegroundWindow($window) | Out-Null
    Start-Sleep 1
    "foreground: $([Win.User32]::GetForegroundWindow() -eq $window)"
    Shot $window "real-$theme-grid"
    RightClick $window 450 150
    Start-Sleep -Milliseconds 1500
    Shot $window "real-$theme-item-menu"
    [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
    Start-Sleep -Milliseconds 800
    [System.Windows.Forms.SendKeys]::SendWait('^t')
    Start-Sleep -Milliseconds 1500
    Shot $window "real-$theme-quick-search"
    [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
    Start-Sleep -Milliseconds 500
    $process.CloseMainWindow() | Out-Null
    if (-not $process.WaitForExit(15000)) { Stop-Process -Id $process.Id -Force }
    Start-Sleep 2
    Remove-Item Env:PICTO_GPUI_STATE, Env:PICTO_GPUI_APP_DATA
}
