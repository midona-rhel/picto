# Clicks the minimise, maximise and close buttons of Picto's main window on
# Windows with the real mouse, and fails when a click does not do what the
# button is for. The job `verify:app` of `.github/workflows/verify.yml` runs
# this script after it has started the app.
#
# The app draws these buttons itself and declares each one to Windows as a
# caption button. This script finds each button by asking the window what is
# at a point (the message WM_NCHITTEST), as Windows itself does, so it does
# not depend on where the app puts the buttons.
param(
    # The id of the app's process.
    [Parameter(Mandatory)] [int] $ProcessId
)
$ErrorActionPreference = 'Stop'

Add-Type -Namespace Win -Name User32 -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out RECT rect);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref POINT point);
[DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
[DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr window);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
'@
[Win.User32]::SetProcessDPIAware() | Out-Null

function Expect($condition, $what) {
    if (-not $condition) { throw "FAILED: $what" }
    "ok: $what"
}

# Waits until the process has a main window, and returns the window's handle.
function MainWindow {
    for ($second = 0; $second -lt 120; $second++) {
        $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
        if (-not $process) { throw 'FAILED: the app ended before its window opened' }
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) { return $process.MainWindowHandle }
        Start-Sleep 1
    }
    throw 'FAILED: the app opened no window in 120 seconds'
}

# Returns the screen position of the middle of the caption button whose
# hit-test code is $code: 8 is the minimise button, 9 the maximise button and
# 20 the close button. It asks the window what is at each point of a row
# that is 24 pixels below the top of the window, from the right edge
# leftwards.
function ButtonCentre($window, $code) {
    $rect = New-Object Win.User32+RECT
    [Win.User32]::GetClientRect($window, [ref] $rect) | Out-Null
    $origin = New-Object Win.User32+POINT
    [Win.User32]::ClientToScreen($window, [ref] $origin) | Out-Null
    $y = $origin.Y + 24
    $found = @()
    for ($x = $origin.X + $rect.Right - 1; $x -ge $origin.X + $rect.Right - 400; $x--) {
        $lparam = [IntPtr] ((($y -band 0xFFFF) -shl 16) -bor ($x -band 0xFFFF))
        $hit = [Win.User32]::SendMessage($window, 0x0084, [IntPtr]::Zero, $lparam).ToInt64()
        if ($hit -eq $code) { $found += $x }
    }
    if ($found.Count -eq 0) { throw "FAILED: no point of the titlebar answers with the hit-test code $code" }
    $centre = [int] (($found | Measure-Object -Average).Average)
    "the button with code $code is $($found.Count) pixels wide, and its middle is at $centre, $y" | Write-Host
    return @($centre, $y)
}

# Moves the mouse to the middle of the button and presses and releases the
# left button there.
function Click($window, $code) {
    $at = ButtonCentre $window $code
    [Win.User32]::SetCursorPos($at[0], $at[1]) | Out-Null
    Start-Sleep -Milliseconds 300
    [Win.User32]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    [Win.User32]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Seconds 2
}

$window = MainWindow
# The library needs a moment to open, and the window must be in front to
# receive the mouse.
Start-Sleep 10
[Win.User32]::SetForegroundWindow($window) | Out-Null
Start-Sleep 1

Expect (-not [Win.User32]::IsZoomed($window)) 'the window is not maximised at the start'
Click $window 9
Expect ([Win.User32]::IsZoomed($window)) 'a click on the maximise button maximises the window'
Click $window 9
Expect (-not [Win.User32]::IsZoomed($window)) 'a second click on it restores the window'
Click $window 8
Expect ([Win.User32]::IsIconic($window)) 'a click on the minimise button minimises the window'
[Win.User32]::ShowWindow($window, 9) | Out-Null
Start-Sleep 2
[Win.User32]::SetForegroundWindow($window) | Out-Null
Start-Sleep 1
Expect (-not [Win.User32]::IsIconic($window)) 'the window is shown again'
Click $window 20
$ended = $false
for ($second = 0; $second -lt 30; $second++) {
    if (-not (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) { $ended = $true; break }
    Start-Sleep 1
}
Expect $ended 'a click on the close button closes the window, and the app ends'
