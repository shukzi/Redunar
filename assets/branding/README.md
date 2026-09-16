# Redunar branding

The canonical application icon is `redunar-logo.svg`: the Comet R mark on a
black rounded-square background. The mark has a deliberate leftward optical
offset to balance the visual weight of the crescent.

Raster exports are provided at 16, 32, 48, 64, 128, 256, 512, and 1024 pixels.
The SVG remains the source of truth for future exports. Render each raster
size directly from the SVG with antialiasing to retain its smooth curves.

The application uses the complete rounded icon beside the existing gradient
**REDUNAR** wordmark. The active Tauri app uses the `com.redunar.Redunar`
desktop/icon identity, derived from `redunar.com`; its native icon is the
existing square export. See [packaging](../../packaging/README.md) before
changing icon registration.
Sidebar, title area, and taskbar should keep the same mark.
