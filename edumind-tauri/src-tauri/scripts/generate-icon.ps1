param(
    [string]$OutputPath = (Join-Path $PSScriptRoot '..\icons\icon.ico')
)

Add-Type -AssemblyName System.Drawing

$directory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Path $directory -Force | Out-Null
$bitmap = [System.Drawing.Bitmap]::new(128, 128)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.Clear([System.Drawing.Color]::FromArgb(17, 24, 39))
$accent = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(79, 70, 229))
$mint = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(45, 212, 191))
$white = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(248, 250, 252))
$graphics.FillEllipse($accent, 10, 10, 108, 108)
$graphics.FillEllipse($mint, 72, 16, 38, 38)
$font = [System.Drawing.Font]::new('Segoe UI', 62, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
$format = [System.Drawing.StringFormat]::new()
$format.Alignment = [System.Drawing.StringAlignment]::Center
$format.LineAlignment = [System.Drawing.StringAlignment]::Center
$graphics.DrawString('E', $font, $white, [System.Drawing.RectangleF]::new(0, 6, 112, 112), $format)
$icon = [System.Drawing.Icon]::FromHandle($bitmap.GetHicon())
$stream = [System.IO.File]::Open($OutputPath, [System.IO.FileMode]::Create)
$icon.Save($stream)
$stream.Dispose()
$icon.Dispose()
$font.Dispose()
$accent.Dispose()
$mint.Dispose()
$white.Dispose()
$graphics.Dispose()
$bitmap.Dispose()
