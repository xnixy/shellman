//! The composition visuals of one glass surface, from the bottom up: the shadow, the blurred
//! backdrop, the tint, and the rim. The backdrop and tint are clipped to one rounded rectangle;
//! the shadow and the rim are shapes of the same rectangle. Everything is in pixels.

use super::style::{Geometry, Style};
use std::cell::OnceCell;
use windows::core::{Interface, Result};
use windows::System::{DispatcherQueue, DispatcherQueueController};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
};
use windows::UI::Color;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBackdropBrush, CompositionColorBrush, CompositionColorGradientStop,
    CompositionRoundedRectangleGeometry, CompositionVisualSurface, Compositor, DropShadow,
    ShapeVisual, SpriteVisual, Visual,
};
use windows_numerics::{Vector2, Vector3};

thread_local! {
    /// One compositor per UI thread, with the dispatcher queue it needs to commit its changes
    /// (the window's own message loop runs that queue). None once it has failed.
    static COMPOSITOR: OnceCell<Option<(Compositor, Option<DispatcherQueueController>)>> =
        const { OnceCell::new() };
}

/// This thread's compositor, made on first use.
pub fn compositor() -> Option<Compositor> {
    COMPOSITOR.with(|cell| {
        cell.get_or_init(|| {
            let queue = match DispatcherQueue::GetForCurrentThread() {
                Ok(_) => None,
                Err(_) => {
                    let options = DispatcherQueueOptions {
                        dwSize: size_of::<DispatcherQueueOptions>() as u32,
                        threadType: DQTYPE_THREAD_CURRENT,
                        apartmentType: DQTAT_COM_NONE,
                    };
                    Some(unsafe { CreateDispatcherQueueController(options) }.ok()?)
                }
            };
            Some((Compositor::new().ok()?, queue))
        })
        .as_ref()
        .map(|(c, _)| c.clone())
    })
}

fn color(c: egui::Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn v3(x: f32, y: f32) -> Vector3 {
    Vector3 { X: x, Y: y, Z: 0.0 }
}

pub struct Tree {
    /// Keeps the visuals on the window.
    _target: DesktopWindowTarget,
    /// The shadow's caster, the clipped body and the rim, each placed at the shape's corner.
    layers: [Visual; 3],
    shadow: DropShadow,
    /// The filled shape, drawn into a surface that both paints the caster and masks its shadow.
    mask_visual: ShapeVisual,
    mask: CompositionVisualSurface,
    mask_fill: CompositionColorBrush,
    backdrop: SpriteVisual,
    host_backdrop: CompositionBackdropBrush,
    solid: CompositionColorBrush,
    tint_visual: SpriteVisual,
    tint: CompositionColorBrush,
    rim_visual: ShapeVisual,
    rim_stops: [CompositionColorGradientStop; 3],
    /// The clip, the shadow's shape, and the rim's (half a pixel in, so its line is crisp).
    shapes: [CompositionRoundedRectangleGeometry; 3],
}

impl Tree {
    /// The visuals for the backdrop window `hwnd`, drawn as its only content.
    pub fn new(compositor: &Compositor, hwnd: HWND) -> Result<Self> {
        let c = compositor;
        let interop: ICompositorDesktopInterop = c.cast()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, true)? };
        let root = c.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        let shapes = [
            c.CreateRoundedRectangleGeometry()?,
            c.CreateRoundedRectangleGeometry()?,
            c.CreateRoundedRectangleGeometry()?,
        ];

        // The shadow: a sprite painted with the filled shape, which also masks its shadow. (A
        // layer visual's shadow follows the layer's rectangle, not its content: measured.) The
        // glass covers the fill, so only the shadow outside the shape shows.
        let mask_visual = c.CreateShapeVisual()?;
        let mask_fill = c.CreateColorBrush()?;
        let mask_shape = c.CreateSpriteShapeWithGeometry(&shapes[1])?;
        mask_shape.SetFillBrush(&mask_fill)?;
        mask_visual.Shapes()?.Append(&mask_shape)?;
        let mask = c.CreateVisualSurface()?;
        mask.SetSourceVisual(&mask_visual)?;
        let mask_brush = c.CreateSurfaceBrushWithSurface(&mask)?;
        let shadow = c.CreateDropShadow()?;
        shadow.SetMask(&mask_brush)?;
        let caster = c.CreateSpriteVisual()?;
        caster.SetBrush(&mask_brush)?;
        caster.SetShadow(&shadow)?;

        let body = c.CreateContainerVisual()?;
        body.SetClip(&c.CreateGeometricClipWithGeometry(&shapes[0])?)?;
        let backdrop = c.CreateSpriteVisual()?;
        let tint_visual = c.CreateSpriteVisual()?;
        let tint = c.CreateColorBrush()?;
        tint_visual.SetBrush(&tint)?;
        body.Children()?.InsertAtTop(&backdrop)?;
        body.Children()?.InsertAtTop(&tint_visual)?;

        // The rim: brightest along the top, fading to a faint line by a third of the way down.
        let rim_visual = c.CreateShapeVisual()?;
        let gradient = c.CreateLinearGradientBrush()?;
        gradient.SetStartPoint(v2(0.0, 0.0))?;
        gradient.SetEndPoint(v2(0.0, 1.0))?;
        let rim_stops = [
            c.CreateColorGradientStop()?,
            c.CreateColorGradientStop()?,
            c.CreateColorGradientStop()?,
        ];
        for (stop, offset) in rim_stops.iter().zip([0.0, 0.3, 1.0]) {
            stop.SetOffset(offset)?;
            gradient.ColorStops()?.Append(stop)?;
        }
        let rim_shape = c.CreateSpriteShapeWithGeometry(&shapes[2])?;
        rim_shape.SetStrokeBrush(&gradient)?;
        rim_shape.SetStrokeThickness(1.0)?;
        rim_visual.Shapes()?.Append(&rim_shape)?;

        let children = root.Children()?;
        children.InsertAtTop(&caster)?;
        children.InsertAtTop(&body)?;
        children.InsertAtTop(&rim_visual)?;
        Ok(Self {
            _target: target,
            layers: [caster.cast()?, body.cast()?, rim_visual.cast()?],
            shadow,
            mask_visual,
            mask,
            mask_fill,
            backdrop,
            host_backdrop: c.CreateHostBackdropBrush()?,
            solid: c.CreateColorBrush()?,
            tint_visual,
            tint,
            rim_visual,
            rim_stops,
            shapes,
        })
    }

    /// Lay the visuals out for `geo` and colour them for `style`. With `effects` off (Windows'
    /// transparency effects are turned off) the backdrop is a solid fill instead of the blur.
    pub fn apply(&self, geo: &Geometry, style: &Style, effects: bool) -> Result<()> {
        let size = v2(geo.shape.0 as f32, geo.shape.1 as f32);
        let r = geo.radius;
        for layer in &self.layers {
            layer.SetOffset(v3(geo.left as f32, geo.top as f32))?;
            layer.SetSize(size)?;
        }
        for visual in [&self.mask_visual, &self.rim_visual] {
            visual.SetSize(size)?;
        }
        self.mask.SetSourceSize(size)?;
        for visual in [&self.backdrop, &self.tint_visual] {
            visual.SetSize(size)?;
        }
        for shape in &self.shapes[..2] {
            shape.SetSize(size)?;
            shape.SetCornerRadius(v2(r, r))?;
        }
        let rim = &self.shapes[2];
        rim.SetOffset(v2(0.5, 0.5))?;
        rim.SetSize(v2(size.X - 1.0, size.Y - 1.0))?;
        rim.SetCornerRadius(v2((r - 0.5).max(0.0), (r - 0.5).max(0.0)))?;

        let [sr, sg, sb, sa] = style.shadow.color.to_srgba_unmultiplied();
        self.shadow.SetColor(Color {
            A: 255,
            R: sr,
            G: sg,
            B: sb,
        })?;
        self.shadow.SetOpacity(sa as f32 / 255.0)?;
        self.shadow.SetBlurRadius(geo.shadow_blur)?;
        self.shadow.SetOffset(v3(0.0, geo.shadow_offset))?;
        // The filled shape under the glass: the tint's colour, so its edge cannot darken the
        // glass's own anti-aliased edge.
        self.mask_fill.SetColor(color(style.tint.to_opaque()))?;

        self.solid.SetColor(color(style.solid))?;
        if effects {
            self.backdrop.SetBrush(&self.host_backdrop)?;
        } else {
            self.backdrop.SetBrush(&self.solid)?;
        }
        self.tint.SetColor(color(style.tint))?;
        let [rr, rg, rb, ra] = style.rim.to_srgba_unmultiplied();
        let faint = Color {
            A: ra / 3,
            R: rr,
            G: rg,
            B: rb,
        };
        let stops = [color(style.rim), faint, faint];
        for (stop, c) in self.rim_stops.iter().zip(stops) {
            stop.SetColor(c)?;
        }
        Ok(())
    }
}
