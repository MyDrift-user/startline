# Draws the logo, the icon and the installer's pictures from the same shape as app_icon_pixels in src/main.rs.
# Run it again after changing that picture: pwsh -File assets\make-assets.ps1
$ErrorActionPreference = 'Stop'
$assets = $PSScriptRoot
$installer = Join-Path $PSScriptRoot '..\installer\windows'
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing, System.Drawing.Primitives, System.Drawing.Common -ErrorAction SilentlyContinue @'
using System; using System.Drawing;
public static class StartlineIcon {
  public static Bitmap Make(int size) {
    var b = new Bitmap(size, size, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
    float s = size, radius = s * 0.22f;
    for (int y = 0; y < size; y++) for (int x = 0; x < size; x++) {
      float fx = x + 0.5f, fy = y + 0.5f;
      float dx = Math.Abs(fx - s / 2) - (s / 2 - radius), dy = Math.Abs(fy - s / 2) - (s / 2 - radius);
      float mx = Math.Max(dx, 0), my = Math.Max(dy, 0);
      float outside = (float)Math.Sqrt(mx * mx + my * my) - radius;
      float alpha = Math.Min(1, Math.Max(0, 0.5f - outside));
      int r = 39, g = 39, bl = 37;
      float u = fx / s, v = fy / s;
      bool left = u >= 0.24 && u <= 0.46, right = u >= 0.54 && u <= 0.76, top = v >= 0.24 && v <= 0.46, bottom = v >= 0.54 && v <= 0.76;
      float du = u - 0.65f, dv = v - 0.65f;
      if ((left && (top || bottom)) || (right && top)) { r = 236; g = 234; bl = 229; }
      if (Math.Sqrt(du * du + dv * dv) <= 0.12) { r = 217; g = 164; bl = 107; }
      b.SetPixel(x, y, Color.FromArgb((int)(alpha * 255), r, g, bl));
    }
    return b;
  }
}
'@

$logo = [StartlineIcon]::Make(256)
$logo.Save((Join-Path $assets 'logo.png'), [Drawing.Imaging.ImageFormat]::Png); $logo.Dispose()

# The icon: PNG frames in an ICO container.
$sizes = 16, 24, 32, 48, 64, 256
$frames = foreach ($size in $sizes) {
  $m = New-Object IO.MemoryStream
  $bmp = [StartlineIcon]::Make($size)
  $bmp.Save($m, [Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  ,$m.ToArray()
}
$ico = New-Object IO.MemoryStream
$w = New-Object IO.BinaryWriter $ico
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
  $d = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
  $w.Write([byte]$d); $w.Write([byte]$d); $w.Write([byte]0); $w.Write([byte]0)
  $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]$frames[$i].Length); $w.Write([uint32]$offset)
  $offset += $frames[$i].Length
}
foreach ($f in $frames) { $w.Write([byte[]]$f) }
$w.Flush()
[IO.File]::WriteAllBytes((Join-Path $assets 'icon.ico'), $ico.ToArray())

# The MSI wizard's pictures. Its text is black, so the part behind text stays white.
$dark = [Drawing.Color]::FromArgb(39, 39, 37)
$banner = New-Object Drawing.Bitmap 493, 58, ([Drawing.Imaging.PixelFormat]::Format24bppRgb)
$g = [Drawing.Graphics]::FromImage($banner); $g.Clear([Drawing.Color]::White)
$tile = [StartlineIcon]::Make(40); $g.DrawImage($tile, 442, 9, 40, 40); $tile.Dispose(); $g.Dispose()
$banner.Save((Join-Path $installer 'banner.bmp'), [Drawing.Imaging.ImageFormat]::Bmp); $banner.Dispose()
$dialog = New-Object Drawing.Bitmap 493, 312, ([Drawing.Imaging.PixelFormat]::Format24bppRgb)
$g = [Drawing.Graphics]::FromImage($dialog); $g.Clear([Drawing.Color]::White)
$brush = New-Object Drawing.SolidBrush $dark; $g.FillRectangle($brush, 0, 0, 164, 312); $brush.Dispose()
$tile = [StartlineIcon]::Make(96); $g.DrawImage($tile, 34, 40, 96, 96); $tile.Dispose(); $g.Dispose()
$dialog.Save((Join-Path $installer 'dialog.bmp'), [Drawing.Imaging.ImageFormat]::Bmp); $dialog.Dispose()
